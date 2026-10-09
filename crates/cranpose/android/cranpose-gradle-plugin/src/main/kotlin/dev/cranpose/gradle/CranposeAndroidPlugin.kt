package dev.cranpose.gradle

import com.android.build.api.artifact.SingleArtifact
import com.android.build.api.dsl.ApplicationExtension
import com.android.build.api.variant.ApplicationAndroidComponentsExtension
import com.android.build.api.variant.ApplicationVariant
import com.android.build.api.variant.FilterConfiguration
import org.gradle.api.GradleException
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.tasks.TaskProvider
import java.io.File
import java.security.MessageDigest
import java.util.HexFormat

/**
 * Configures an Android application built on Cranpose.
 *
 * The plugin owns what every Cranpose application's build would otherwise
 * repeat: building the Rust `cdylib` with `cargo-ndk`, choosing ABIs and Cargo
 * profiles, packaging the resulting `.so` files, contributing the framework's
 * Java, manifest entries and ProGuard rules directly (plus, for the optional
 * service modules an application asked for, their own Java, manifest entries
 * and third-party dependencies), and supplying the manifest metadata the
 * activity declaration reads.
 *
 * Nothing here is resolved from a Maven repository: every contribution reads
 * straight out of this crate's `android/` directory, which this build sits
 * inside (see `androidRoot()` and the plugin's own `build.gradle.kts`).
 *
 * An application's build file then reads:
 *
 * ```kotlin
 * plugins {
 *     id("com.android.application")
 *     id("dev.cranpose.android")
 * }
 *
 * cranpose {
 *     cargoPackage.set("my-app-platform")
 *     services.add("notifications")
 * }
 * ```
 */
class CranposeAndroidPlugin : Plugin<Project> {

    override fun apply(project: Project) {
        val cranpose = project.extensions.create("cranpose", CranposeExtension::class.java)
        applyDefaults(project, cranpose)

        project.plugins.withId("com.android.application") {
            configureApplication(project, cranpose)
        }
        project.afterEvaluate {
            if (!project.plugins.hasPlugin("com.android.application")) {
                throw GradleException(
                    "the dev.cranpose.android plugin configures an Android application; " +
                        "apply com.android.application alongside it"
                )
            }
        }
    }

    private fun applyDefaults(project: Project, cranpose: CranposeExtension) {
        cranpose.workspaceRoot.convention("../../../..")
        cranpose.features.convention(listOf("android", "renderer-wgpu"))
        cranpose.defaultFeatures.convention(false)
        cranpose.debugAbis.convention(listOf("x86_64"))
        cranpose.releaseAbis.convention(
            project.providers.gradleProperty(RELEASE_ABIS_PROPERTY).map { value ->
                value.split(',').map(String::trim).filter(String::isNotEmpty)
            }.orElse(listOf("arm64-v8a"))
        )
        cranpose.releaseProfile.convention("release")
        cranpose.debugProfile.convention("dev")
        cranpose.debugAbiFeatures.convention(emptyMap())
        cranpose.releaseAbiFeatures.convention(emptyMap())
        cranpose.services.convention(emptySet())
        cranpose.requiredFeatures.convention(emptySet())
        cranpose.environment.convention(emptyMap())
        cranpose.label.convention(project.rootProject.name)
        cranpose.theme.convention("@android:style/Theme.NoTitleBar.Fullscreen")
        cranpose.libraryName.convention(
            cranpose.cargoPackage.map { name -> name.replace('-', '_') }
        )
    }

    /**
     * This crate's `android/` directory: the parent of the directory the
     * plugin's own build lives in, captured at that build's configuration time
     * (see `cranposeAndroidRootResource` in the plugin's `build.gradle.kts`) and
     * read here as a classpath resource.
     *
     * This is always correct because the plugin is never fetched as a prebuilt
     * artifact: a consuming application's `settings.gradle.kts` locates this
     * crate's source with `cargo metadata` and `includeBuild`s the plugin from
     * it, so the plugin is recompiled from that same tree on every build.
     */
    private fun androidRoot(): File =
        File(
            CranposeAndroidPlugin::class.java.getResourceAsStream(ANDROID_ROOT_RESOURCE)
                ?.use { stream -> stream.readBytes().toString(Charsets.UTF_8).trim() }
                ?.takeIf { path -> path.isNotEmpty() }
                ?: throw GradleException(
                    "the Cranpose Gradle plugin carries no $ANDROID_ROOT_RESOURCE"
                )
        )

    private fun configureApplication(project: Project, cranpose: CranposeExtension) {
        val androidComponents =
            project.extensions.getByType(ApplicationAndroidComponentsExtension::class.java)
        var minimumVariantSdk: Int? = null
        cranpose.androidApiLevel.convention(
            project.providers.provider<Int> {
                minimumVariantSdk
                    ?: project.extensions.getByType(ApplicationExtension::class.java).defaultConfig.minSdk
            }
        )

        // `finalizeDsl` runs after the application's own `android { }` and
        // `cranpose { }` blocks and before the Android plugin locks its DSL,
        // which is the only window in which both are true.
        androidComponents.finalizeDsl { android ->
            configureAndroid(cranpose, android)
        }

        // Contributes the framework's Java, manifest entries and ProGuard
        // rules directly to each variant, the way a library module's AAR would
        // -- except read straight from this crate's source instead of resolved
        // from a repository.
        androidComponents.onVariants { variant ->
            val minSdk = variant.minSdk.apiLevel
            minimumVariantSdk = minOf(minimumVariantSdk ?: minSdk, minSdk)
            val native = registerVariantNativeBuild(project, cranpose, variant)
            variant.sources.jniLibs?.addGeneratedSourceDirectory(native, CranposeNativeBuild::outputDir)
            contributeCranposeSources(project, cranpose, variant, native)
            checkManifest(project, cranpose, variant, native)
            registerPipelineRecording(project, androidComponents, variant)
        }

        project.afterEvaluate {
            requireCargoPackage(cranpose)
            requireWorkspace(project, cranpose)
            addDependencies(project, cranpose)
        }
    }

    /**
     * Registers `cranposeRecordPipelines<Variant>`, which installs the variant
     * and records the pipelines it draws on each connected device into the
     * application's `assets/cranpose_gpu/`, for its fresh installs.
     */
    private fun registerPipelineRecording(
        project: Project,
        androidComponents: ApplicationAndroidComponentsExtension,
        variant: ApplicationVariant,
    ) {
        val name = variant.name.replaceFirstChar { first -> first.uppercase() }
        project.tasks.register("cranposeRecordPipelines$name", CranposeRecordPipelines::class.java) {
            description =
                "Records the pipelines ${variant.name} draws on each connected device, for fresh installs"
            group = "cranpose"
            dependsOn("install$name")
            adb.set(androidComponents.sdkComponents.adb)
            applicationId.set(variant.applicationId)
            drawSeconds.set(10)
            seedDir.set(project.layout.projectDirectory.dir("src/main/assets/cranpose_gpu"))
            outputs.upToDateWhen { false }
        }
    }

    /**
     * Adds the base framework's Java source directory, manifest and ProGuard
     * rules to this variant, and the same for every service the application
     * asked for through `cranpose { services }`.
     *
     * Static (pre-existing, hand-written) content is added by absolute path
     * rather than copied into the build directory: these are the framework's
     * real source files, and an application inspecting or stepping into them
     * should land on the file this crate ships, not a build-generated copy.
     */
    private fun contributeCranposeSources(
        project: Project,
        cranpose: CranposeExtension,
        variant: ApplicationVariant,
        native: TaskProvider<CranposeNativeBuild>,
    ) {
        val root = androidRoot()
        contributeManifest(variant, root, "base")
        variant.sources.java?.addStaticSourceDirectory(File(root, "java").absolutePath)
        variant.sources.java?.addStaticSourceDirectory(File(root, "java-webview").absolutePath)
        variant.proguardFiles.add(
            project.layout.projectDirectory.file(File(root, "proguard-rules.pro").absolutePath)
        )

        for (service in requireKnownServices(cranpose)) {
            contributeManifest(variant, root, service)
            SERVICE_JAVA_SOURCE[service]?.let { dir ->
                variant.sources.java?.addStaticSourceDirectory(File(root, dir).absolutePath)
            }
        }

        val name = variant.name.replaceFirstChar { first -> first.uppercase() }
        val declared = project.tasks.register(
            "cranpose${name}DeclaredSources",
            CranposeDeclaredSources::class.java,
        ) {
            description = "Copies the framework Java for the services ${variant.name} declares"
            declaration.from(native.flatMap(CranposeNativeBuild::declarationDir).map { directory ->
                directory.asFileTree
            })
            serviceSources.set(
                DECLARED_SERVICES.mapValues { (_, service) ->
                    File(root, service.javaSource).absolutePath
                }
            )
            sources.from(DECLARED_SERVICES.values.map { service -> File(root, service.javaSource) })
        }
        variant.sources.java?.addGeneratedSourceDirectory(
            declared,
            CranposeDeclaredSources::outputDir,
        )
    }

    /**
     * Adds a service's manifest when it has one. A service that only needs a
     * permission has none: the permission is the application's to declare.
     */
    private fun contributeManifest(variant: ApplicationVariant, root: File, name: String) {
        val manifest = File(root, "manifests/$name.xml")
        if (manifest.isFile) {
            variant.sources.manifests.addStaticManifestFile(manifest.absolutePath)
        }
    }

    /**
     * Checks the merged manifest: every permission the application's services
     * need is declared in it, and every hardware feature those permissions
     * carry is either optional or named by `cranpose { requiredFeatures }`.
     *
     * This reads the merged manifest rather than the framework's own fragments
     * because a permission from the application itself, or from any library it
     * depends on, reaches users and costs devices just the same.
     */
    private fun checkManifest(
        project: Project,
        cranpose: CranposeExtension,
        variant: ApplicationVariant,
        native: TaskProvider<CranposeNativeBuild>,
    ) {
        val name = variant.name.replaceFirstChar { first -> first.uppercase() }
        val task = project.tasks.register(
            "cranpose${name}ManifestCheck",
            CranposeManifestCheck::class.java,
        )
        val needed = requireKnownServices(cranpose).flatMap { service ->
            SERVICE_PERMISSIONS[service].orEmpty().map { permission -> permission to service }
        }.toMap()
        task.configure {
            description = "Checks ${variant.name}'s permissions and the features they carry"
            requiredFeatures.set(cranpose.requiredFeatures)
            servicePermissions.set(needed)
            declaration.from(native.flatMap(CranposeNativeBuild::declarationDir).map { directory ->
                directory.asFileTree
            })
        }
        variant.artifacts
            .use(task)
            .wiredWithFiles(
                CranposeManifestCheck::mergedManifest,
                CranposeManifestCheck::updatedManifest,
            )
            .toTransform(SingleArtifact.MERGED_MANIFEST)
    }

    private fun requireCargoPackage(cranpose: CranposeExtension): String =
        cranpose.cargoPackage.orNull
            ?: throw GradleException(
                "cranpose { cargoPackage } must name the Cargo package that builds this " +
                    "application's cdylib"
            )

    private fun requireWorkspace(project: Project, cranpose: CranposeExtension): File {
        val workspace = project.file(cranpose.workspaceRoot.get()).canonicalFile
        if (!File(workspace, "Cargo.toml").isFile) {
            throw GradleException(
                "cranpose { workspaceRoot } resolved to $workspace, which holds no Cargo.toml"
            )
        }
        return workspace
    }

    private fun configureAndroid(
        cranpose: CranposeExtension,
        android: ApplicationExtension,
    ) {
        requireCargoPackage(cranpose)

        // The library's activity declaration reads these, so an application
        // never repeats the activity, the lib_name, or the launcher filter.
        android.defaultConfig.manifestPlaceholders["cranposeLibName"] = cranpose.libraryName.get()
        android.defaultConfig.manifestPlaceholders["cranposeLabel"] = cranpose.label.get()
        android.defaultConfig.manifestPlaceholders["cranposeTheme"] = cranpose.theme.get()

        configureAbis(cranpose, android)

        // A `release-fast` build keeps its symbols, because that profile exists
        // to be profiled and crash-reported on a real device.
        if (cranpose.releaseProfile.get() != "release") {
            android.packaging.jniLibs.keepDebugSymbols.add(
                "**/lib${cranpose.libraryName.get()}.so"
            )
        }
    }

    private fun usesDebugSettings(buildType: String?, debuggable: Boolean): Boolean =
        when (buildType) {
            "debug" -> true
            "release" -> false
            else -> debuggable
        }

    private fun configureAbis(cranpose: CranposeExtension, android: ApplicationExtension) {
        val split = android.splits.abi
        if (split.isEnable) {
            val abis = android.buildTypes.flatMap { type ->
                if (usesDebugSettings(type.name, type.isDebuggable)) {
                    cranpose.debugAbis.get()
                } else {
                    cranpose.releaseAbis.get()
                }
            }.distinct()
            split.reset()
            split.include(*abis.toTypedArray())
        } else {
            android.buildTypes.forEach { type ->
                val abis = if (usesDebugSettings(type.name, type.isDebuggable)) {
                    cranpose.debugAbis.get()
                } else {
                    cranpose.releaseAbis.get()
                }
                type.ndk.abiFilters.addAll(abis)
            }
        }
    }

    private fun addDependencies(project: Project, cranpose: CranposeExtension) {
        for (service in requireKnownServices(cranpose)) {
            for (coordinate in SERVICE_DEPENDENCIES[service].orEmpty()) {
                project.dependencies.add("implementation", coordinate)
            }
        }
    }

    private fun requireKnownServices(cranpose: CranposeExtension): Set<String> {
        val services = cranpose.services.get()
        val unknown = services - KNOWN_SERVICES
        if (unknown.isNotEmpty()) {
            throw GradleException(
                "cranpose { services } does not know ${unknown.joinToString(", ")}; " +
                    "valid names are ${KNOWN_SERVICES.joinToString(", ")}"
            )
        }
        return services
    }

    private fun registerVariantNativeBuild(
        project: Project,
        cranpose: CranposeExtension,
        variant: ApplicationVariant,
    ): TaskProvider<CranposeNativeBuild> {
        val debug = usesDebugSettings(variant.buildType, variant.debuggable)
        val settings = if (debug) "Debug" else "Release"
        val abis = if (debug) cranpose.debugAbis.get() else cranpose.releaseAbis.get()
        val abiFeatures = if (debug) cranpose.debugAbiFeatures.get() else cranpose.releaseAbiFeatures.get()
        val profile = if (debug) cranpose.debugProfile.get() else cranpose.releaseProfile.get()
        val groups = nativeBuildGroups(cranpose, settings, abis, abiFeatures)
        val cargoPackage = requireCargoPackage(cranpose)
        val workspace = requireWorkspace(project, cranpose)
        val name = variant.name.replaceFirstChar { it.uppercase() }
        val cargo = project.gradle.sharedServices.registerIfAbsent(
            "cranposeNativeBuilds",
            CranposeCargoBuildService::class.java,
        ) {
            maxParallelUsages.set(1)
        }
        for (output in variant.outputs) {
            val abi = output.filters.find { it.filterType == FilterConfiguration.FilterType.ABI }
            if (abi != null && abi.identifier !in abis) {
                output.enabled.set(false)
            }
        }
        return project.tasks.register("cranposeBuildNative$name", CranposeNativeBuild::class.java) {
            description = "Builds ${variant.name}'s Rust library for ${abis.joinToString(", ")}"
            group = "cranpose"
            outputDir.set(project.layout.buildDirectory.dir("generated/cranpose/${variant.name}/jniLibs"))
            declarationDir.set(project.layout.buildDirectory.dir("generated/cranpose/${variant.name}/capabilities"))
            outputs.upToDateWhen { false }
            usesService(cargo)
            doLast {
                requireCargoNdk(project)
                val nativeOutput = outputDir.get().asFile
                val stagedDeclarations = declarationDir.get().asFile
                for (output in listOf(nativeOutput, stagedDeclarations)) {
                    if (output.exists() && !output.deleteRecursively()) {
                        throw GradleException("Cannot clear native output ${output.absolutePath}")
                    }
                }
                stagedDeclarations.mkdirs()
                val api = cranpose.androidApiLevel.orNull
                val linkArguments = if (debug) {
                    emptyList()
                } else {
                    val unwindTableDiscard = File(temporaryDir, "discard-unwind-tables.ld")
                    unwindTableDiscard.writeText(DISCARD_UNWIND_TABLES)
                    releaseLinkArguments(api, unwindTableDiscard)
                }
                val defaultFeatures = cranpose.defaultFeatures.get()
                val environment = cranpose.environment.get().filterKeys { it != CAPABILITIES_DIR_VARIABLE }
                for (pass in groups) {
                    for (abi in pass.abis) {
                        val configuration = listOf(
                            "package=$cargoPackage", "profile=$profile", "abi=$abi",
                            "api=$api", "defaultFeatures=$defaultFeatures",
                        ) + pass.features.sorted().map { "feature=$it" } +
                            environment.toSortedMap().map { (key, value) -> "env=$key=$value" }
                        val key = HexFormat.of().formatHex(
                            MessageDigest.getInstance("SHA-256").digest(
                                configuration.joinToString("\u0000").toByteArray(Charsets.UTF_8)
                            )
                        )
                        val declarations = File(workspace, "target/cranpose/android/$cargoPackage/$key")
                        declarations.mkdirs()
                        val arguments = mutableListOf("ndk")
                        api?.let { level -> arguments += listOf("--platform", level.toString()) }
                        arguments += listOf("-t", abi, "-o", nativeOutput.absolutePath, "rustc", "-p", cargoPackage, "--lib")
                        if (profile != "dev") {
                            arguments += listOf("--profile", profile)
                        }
                        if (!defaultFeatures) {
                            arguments += "--no-default-features"
                        }
                        if (pass.features.isNotEmpty()) {
                            arguments += listOf("--features", pass.features.joinToString(","))
                        }
                        if (linkArguments.isNotEmpty()) {
                            arguments += "--"
                            arguments += linkArguments.flatMap { listOf("-C", "link-arg=$it") }
                        }
                        logger.lifecycle(
                            "cranpose: building $cargoPackage for $abi " +
                                "with the $profile profile and ${pass.features.joinToString(",")}"
                        )
                        execOperations.exec {
                            workingDir = workspace
                            commandLine(listOf("cargo") + arguments)
                            environment(environment)
                            environment(CAPABILITIES_DIR_VARIABLE, declarations.absolutePath)
                        }
                        val declaration = File(declarations, "$cargoPackage-capabilities.json")
                        if (declaration.isFile) {
                            declaration.copyTo(File(stagedDeclarations, "$abi.json"), overwrite = true)
                        }
                    }
                }
            }
        }
    }

    private data class NativeBuildGroup(val abis: List<String>, val features: List<String>)

    private fun nativeBuildGroups(
        cranpose: CranposeExtension,
        variant: String,
        abis: List<String>,
        abiFeatures: Map<String, List<String>>,
    ): List<NativeBuildGroup> {
        val base = cranpose.features.get()
        val unbuilt = abiFeatures.keys.filterNot { abi -> abi in abis }
        if (unbuilt.isNotEmpty()) {
            throw GradleException(
                "cranpose { ${variant.lowercase()}AbiFeatures } names " +
                    "${unbuilt.joinToString(", ")}, which ${variant.lowercase()}Abis does not " +
                    "build; those features would never be enabled"
            )
        }
        return abis
            .groupBy { abi -> (base + abiFeatures[abi].orEmpty()).distinct() }
            .map { (features, grouped) -> NativeBuildGroup(grouped, features) }
    }

    private fun requireCargoNdk(project: Project) {
        val result = project.providers.exec {
            commandLine("cargo", "ndk", "--version")
            isIgnoreExitValue = true
        }.result.get()
        if (result.exitValue != 0) {
            throw GradleException(
                "cargo-ndk is required to build the Cranpose native library. " +
                    "Install it with `cargo install cargo-ndk`."
            )
        }
    }

    /**
     * What a release library is linked with, passed to the final crate alone
     * so an application's own rustflags still apply: identical functions
     * folded into one (Rust promises no function a unique address), no unwind
     * tables (release builds abort on panic, so nothing unwinds), and, on
     * Android 6.0 and later, which read them, relocations in Android's packed
     * format.
     */
    private fun releaseLinkArguments(api: Int?, unwindTableDiscard: File): List<String> =
        listOf("-Wl,--icf=all", "-Wl,--no-eh-frame-hdr", "-T${unwindTableDiscard.absolutePath}") +
            listOfNotNull("-Wl,--pack-dyn-relocs=android".takeIf { api != null && api >= PACKED_RELOCATIONS_MIN_API })

    private companion object {
        /**
         * Linker script dropping every unwind table, the prebuilt standard
         * library's included, which a codegen flag on the final crate cannot
         * reach. `INSERT` keeps the default layout.
         */
        const val DISCARD_UNWIND_TABLES = """SECTIONS {
  /DISCARD/ : {
    *(.eh_frame) *(.gcc_except_table .gcc_except_table.*)
    *(.ARM.exidx .ARM.exidx.*) *(.ARM.extab .ARM.extab.*)
  }
} INSERT AFTER .text;
"""

        /** The first Android release whose loader reads packed relocations. */
        const val PACKED_RELOCATIONS_MIN_API = 23

        /**
         * Gradle property naming the architectures a release build produces,
         * comma separated.
         *
         * The default is the one architecture a development device runs.
         * Every architecture a release carries is one more full native build
         * of the workspace, run one after another, so a build that only has
         * to prove the application assembles asks for none of them.
         */
        const val RELEASE_ABIS_PROPERTY = "cranposeReleaseAbis"

        /** Names the directory a build script writes the declaration into. */
        const val CAPABILITIES_DIR_VARIABLE = "CRANPOSE_CAPABILITIES_DIR"

        val KNOWN_SERVICES = setOf(
            "background",
            "billing",
            "camera",
            "haptics",
            "media",
            "network",
            "notifications",
            "overlay",
            "wearable",
        )

        /**
         * Extra Java source directories a service contributes, relative to
         * `androidRoot()`. Most services are manifest-only; `billing` also
         * carries `CranposeBilling`, which needs the Play Billing library, and
         * `wearable` carries `CranposeWearable`, which needs Google Play
         * services' wearable library, so both live outside the base `java/`
         * every application compiles.
         */
        val SERVICE_JAVA_SOURCE = mapOf(
            "billing" to "java-billing",
            "wearable" to "java-wearable",
        )

        /** Third-party dependencies a service needs beyond the framework's own. */
        val SERVICE_DEPENDENCIES = mapOf(
            "billing" to listOf("com.android.billingclient:billing:9.1.0"),
            "wearable" to listOf("com.google.android.gms:play-services-wearable:20.0.1"),
        )

        /**
         * The permissions each service needs to work, which the application
         * declares in its own manifest.
         *
         * The framework declares none of them. A permission is a line in the
         * store listing and a question to the person holding the phone, so it
         * belongs to the application that shows it, written where anyone
         * reading that application can see it. What the framework does instead
         * is refuse to build when a service is used and its permission is not
         * there, so the code path does not fail silently on a device.
         */
        val SERVICE_PERMISSIONS = mapOf(
            "background" to listOf(
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_DATA_SYNC",
            ),
            "billing" to listOf("com.android.vending.BILLING"),
            "camera" to listOf("android.permission.CAMERA"),
            "haptics" to listOf("android.permission.VIBRATE"),
            "media" to listOf(
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK",
            ),
            "network" to listOf(
                "android.permission.INTERNET",
                "android.permission.ACCESS_NETWORK_STATE",
            ),
            "notifications" to listOf("android.permission.POST_NOTIFICATIONS"),
            "overlay" to listOf("android.permission.SYSTEM_ALERT_WINDOW"),
        )

        /** Written by this plugin's build, relative to this class's package. */
        const val ANDROID_ROOT_RESOURCE = "android-root.txt"
    }
}
