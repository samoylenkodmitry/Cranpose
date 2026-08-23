package dev.cranpose.gradle

import com.android.build.api.dsl.ApplicationExtension
import com.android.build.api.variant.ApplicationAndroidComponentsExtension
import com.android.build.api.variant.ApplicationVariant
import org.gradle.api.GradleException
import org.gradle.api.Plugin
import org.gradle.api.Project
import org.gradle.api.Task
import org.gradle.api.tasks.Exec
import org.gradle.api.tasks.TaskProvider
import java.io.File

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
        val continuousIntegration =
            project.providers.environmentVariable("CI").isPresent ||
                project.providers.environmentVariable("GITHUB_ACTIONS").isPresent

        cranpose.workspaceRoot.convention("../../../..")
        cranpose.features.convention(listOf("android", "renderer-wgpu"))
        cranpose.defaultFeatures.convention(false)
        cranpose.debugAbis.convention(listOf("x86_64"))
        cranpose.releaseAbis.convention(
            if (continuousIntegration) {
                listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
            } else {
                listOf("arm64-v8a")
            }
        )
        cranpose.releaseProfile.convention("release")
        cranpose.debugProfile.convention("dev")
        cranpose.debugAbiFeatures.convention(emptyMap())
        cranpose.releaseAbiFeatures.convention(emptyMap())
        cranpose.services.convention(emptySet())
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

        // `finalizeDsl` runs after the application's own `android { }` and
        // `cranpose { }` blocks and before the Android plugin locks its DSL,
        // which is the only window in which both are true.
        androidComponents.finalizeDsl { android ->
            configureAndroid(cranpose, nativeOutput(project, cranpose), android)
        }

        // Contributes the framework's Java, manifest entries and ProGuard
        // rules directly to each variant, the way a library module's AAR would
        // -- except read straight from this crate's source instead of resolved
        // from a repository.
        androidComponents.onVariants { variant ->
            contributeCranposeSources(project, cranpose, variant)
        }

        project.afterEvaluate {
            val cargoPackage = requireCargoPackage(cranpose)
            val workspace = requireWorkspace(project, cranpose)
            addDependencies(project, cranpose)
            registerNativeBuilds(
                project,
                cranpose,
                cargoPackage,
                workspace,
                nativeOutput(project, cranpose),
            )
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
    ) {
        val root = androidRoot()
        contributeManifest(variant, root, "base")
        variant.sources.java?.addStaticSourceDirectory(File(root, "java").absolutePath)
        variant.proguardFiles.add(
            project.layout.projectDirectory.file(File(root, "proguard-rules.pro").absolutePath)
        )

        for (service in requireKnownServices(cranpose)) {
            contributeManifest(variant, root, service)
            SERVICE_JAVA_SOURCE[service]?.let { dir ->
                variant.sources.java?.addStaticSourceDirectory(File(root, dir).absolutePath)
            }
        }
    }

    private fun contributeManifest(variant: ApplicationVariant, root: File, name: String) {
        variant.sources.manifests.addStaticManifestFile(File(root, "manifests/$name.xml").absolutePath)
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

    /** Where `cargo ndk` writes the ABI directories the APK packages. */
    private fun nativeOutput(project: Project, cranpose: CranposeExtension): File =
        File(requireWorkspace(project, cranpose), "target/android")

    private fun configureAndroid(
        cranpose: CranposeExtension,
        nativeOutput: File,
        android: ApplicationExtension,
    ) {
        requireCargoPackage(cranpose)

        // The library's activity declaration reads these, so an application
        // never repeats the activity, the lib_name, or the launcher filter.
        // `cargo-ndk` links against API 21 unless told otherwise, and that
        // sysroot is missing libraries an application's minSdk guarantees.
        // The application already stated the level it supports; taking it from
        // there is what keeps the two from disagreeing.
        android.defaultConfig.minSdk?.let { minSdk ->
            cranpose.androidApiLevel.convention(minSdk)
        }

        android.defaultConfig.manifestPlaceholders["cranposeLibName"] = cranpose.libraryName.get()
        android.defaultConfig.manifestPlaceholders["cranposeLabel"] = cranpose.label.get()
        android.defaultConfig.manifestPlaceholders["cranposeTheme"] = cranpose.theme.get()

        android.sourceSets.getByName("debug").jniLibs.directories.add(nativeOutput.absolutePath)
        android.sourceSets.getByName("release").jniLibs.directories.add(nativeOutput.absolutePath)

        configureAbis(cranpose, android)

        // A `release-fast` build keeps its symbols, because that profile exists
        // to be profiled and crash-reported on a real device.
        if (cranpose.releaseProfile.get() != "release") {
            android.packaging.jniLibs.keepDebugSymbols.add(
                "**/lib${cranpose.libraryName.get()}.so"
            )
        }
    }

    /**
     * Constrains packaging to the architectures the native build produces.
     *
     * `cranpose { releaseAbis }` is the single statement of which architectures
     * a release carries, so it drives packaging in both shapes an application
     * can take. One APK per architecture is expressed as an ABI split, and AGP
     * refuses to have both a split and `ndk.abiFilters`; the split's own filter
     * list is therefore written from the same value. An application that splits
     * says only that it does — `isEnable` and `isUniversalApk` — and never
     * repeats the architectures, which is what stops it from splitting off an
     * APK the native build never produced a library for.
     */
    private fun configureAbis(cranpose: CranposeExtension, android: ApplicationExtension) {
        val releaseAbis = cranpose.releaseAbis.get()
        val split = android.splits.abi
        if (split.isEnable) {
            split.reset()
            split.include(*releaseAbis.toTypedArray())
            return
        }

        android.buildTypes.getByName("debug").ndk.abiFilters.addAll(cranpose.debugAbis.get())
        android.buildTypes.getByName("release").ndk.abiFilters.addAll(releaseAbis)
    }

    /** `CranposeActivity` extends `androidx.appcompat.app.AppCompatActivity`. */
    private fun addDependencies(project: Project, cranpose: CranposeExtension) {
        project.dependencies.add("implementation", "androidx.appcompat:appcompat:1.7.1")
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

    private fun registerNativeBuilds(
        project: Project,
        cranpose: CranposeExtension,
        cargoPackage: String,
        workspace: File,
        nativeOutput: File,
    ) {
        val debug = registerVariantNativeBuild(
            project,
            variant = "Debug",
            abis = cranpose.debugAbis.get(),
            abiFeatures = cranpose.debugAbiFeatures.get(),
            profile = cranpose.debugProfile.get(),
            cranpose = cranpose,
            cargoPackage = cargoPackage,
            workspace = workspace,
            nativeOutput = nativeOutput,
        )
        val release = registerVariantNativeBuild(
            project,
            variant = "Release",
            abis = cranpose.releaseAbis.get(),
            abiFeatures = cranpose.releaseAbiFeatures.get(),
            profile = cranpose.releaseProfile.get(),
            cranpose = cranpose,
            cargoPackage = cargoPackage,
            workspace = workspace,
            nativeOutput = nativeOutput,
        )

        // Both merge tasks read the native output: `mergeJniLibFolders`
        // collects the source directories and `mergeNativeLibs` collects the
        // libraries inside them. Wiring only the second leaves the first racing
        // the Cargo build, and the APK silently ships the previous build.
        project.tasks.matching { task ->
            task.name.startsWith("merge") &&
                (task.name.contains("NativeLibs") || task.name.contains("JniLibFolders"))
        }.configureEach {
            when {
                name.contains("Debug", ignoreCase = true) -> dependsOn(debug)
                name.contains("Release", ignoreCase = true) -> dependsOn(release)
            }
        }
    }

    /**
     * Registers the Cargo work one variant needs, as one task per feature set.
     *
     * Architectures usually share a feature set and a single `cargo ndk` pass
     * builds them all. They do not always: a native dependency can be
     * unbuildable on one architecture, and the application then ships that
     * architecture without the feature rather than not at all. Grouping by the
     * effective feature set expresses both cases with the same code, and the
     * `cranposeBuildNative<Variant>` task the packaging depends on stays one
     * name however many passes it takes.
     */
    private fun registerVariantNativeBuild(
        project: Project,
        variant: String,
        abis: List<String>,
        abiFeatures: Map<String, List<String>>,
        profile: String,
        cranpose: CranposeExtension,
        cargoPackage: String,
        workspace: File,
        nativeOutput: File,
    ): TaskProvider<out Task> {
        val groups = nativeBuildGroups(cranpose, variant, abis, abiFeatures)
        val passes = groups.mapIndexed { index, group ->
            registerNativeBuild(
                project,
                // A single pass keeps the plain name; several are numbered in
                // the order the architectures were declared.
                taskName = "cranposeBuildNative$variant" +
                    if (groups.size == 1) "" else "Pass${index + 1}",
                description = "Builds this application's Rust cdylib for Android " +
                    "(${variant.lowercase()}: ${group.abis.joinToString(", ")})",
                pass = group,
                variantAbis = abis,
                profile = profile,
                cranpose = cranpose,
                cargoPackage = cargoPackage,
                workspace = workspace,
                nativeOutput = nativeOutput,
            )
        }
        if (passes.size == 1) {
            return passes.single()
        }
        return project.tasks.register("cranposeBuildNative$variant") {
            this.description =
                "Builds this application's Rust cdylib for Android (${variant.lowercase()} ABIs)"
            this.group = "cranpose"
            dependsOn(passes)
        }
    }

    /** One Cargo pass: the architectures that share a feature set. */
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

    private fun registerNativeBuild(
        project: Project,
        taskName: String,
        description: String,
        pass: NativeBuildGroup,
        variantAbis: List<String>,
        profile: String,
        cranpose: CranposeExtension,
        cargoPackage: String,
        workspace: File,
        nativeOutput: File,
    ): TaskProvider<Exec> = project.tasks.register(taskName, Exec::class.java) {
        this.description = description
        this.group = "cranpose"
        workingDir = workspace

        // Cargo is authoritative about what changed, so the task always runs
        // and lets Cargo decide. The output directory is still declared: without
        // it the packaging tasks read a pre-Cargo snapshot, report up to date,
        // and ship the previous build's library — a failure that looks like a
        // device bug rather than a build bug.
        outputs.dir(nativeOutput)
        outputs.upToDateWhen { false }

        val arguments = mutableListOf("ndk")
        cranpose.androidApiLevel.orNull?.let { level ->
            arguments += listOf("--platform", level.toString())
        }
        for (abi in pass.abis) {
            arguments += listOf("-t", abi)
        }
        arguments += listOf("-o", nativeOutput.absolutePath, "build", "-p", cargoPackage, "--lib")
        if (profile != "dev") {
            arguments += listOf("--profile", profile)
        }
        if (!cranpose.defaultFeatures.get()) {
            arguments += "--no-default-features"
        }
        if (pass.features.isNotEmpty()) {
            arguments += listOf("--features", pass.features.joinToString(","))
        }
        commandLine(listOf("cargo") + arguments)
        for ((name, value) in cranpose.environment.get()) {
            environment(name, value)
        }

        doFirst {
            requireCargoNdk(project)
            // `cargo ndk` writes one directory per ABI and removes none, so a
            // build for a different ABI than last time would leave the previous
            // one's library here to be packaged alongside the new one — an APK
            // carrying a stale library for an architecture nobody rebuilt. The
            // whole variant's list is what survives, not this pass's: a second
            // pass must not delete what the first one just wrote.
            clearStaleAbis(nativeOutput, variantAbis)
            project.logger.lifecycle(
                "cranpose: building $cargoPackage for ${pass.abis.joinToString(", ")} " +
                    "with the $profile profile and ${pass.features.joinToString(",")}"
            )
        }
    }

    /** Removes the ABI directories this build is not about to rewrite. */
    private fun clearStaleAbis(nativeOutput: File, abis: List<String>) {
        val current = abis.toSet()
        nativeOutput.listFiles()
            ?.filter { entry -> entry.isDirectory && entry.name !in current }
            ?.forEach { stale -> stale.deleteRecursively() }
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

    private companion object {
        val KNOWN_SERVICES = setOf(
            "background",
            "billing",
            "camera",
            "haptics",
            "media",
            "notifications",
            "overlay",
            "update",
        )

        /**
         * Extra Java source directories a service contributes, relative to
         * `androidRoot()`. Most services are manifest-only; `billing` also
         * carries `CranposeBilling`, which needs the Play Billing library and
         * so lives outside the base `java/` every application compiles.
         */
        val SERVICE_JAVA_SOURCE = mapOf(
            "billing" to "java-billing",
        )

        /** Third-party dependencies a service needs beyond the framework's own. */
        val SERVICE_DEPENDENCIES = mapOf(
            "billing" to listOf("com.android.billingclient:billing:9.1.0"),
        )

        /** Written by this plugin's build, relative to this class's package. */
        const val ANDROID_ROOT_RESOURCE = "android-root.txt"
    }
}
