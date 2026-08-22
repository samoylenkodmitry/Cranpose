package dev.cranpose.gradle

import org.gradle.api.model.ObjectFactory
import org.gradle.api.provider.ListProperty
import org.gradle.api.provider.MapProperty
import org.gradle.api.provider.Property
import org.gradle.api.provider.SetProperty
import org.gradle.kotlin.dsl.mapProperty
import javax.inject.Inject

/**
 * How this application is built on Cranpose.
 *
 * Everything has a default that is right for an ordinary application, so a
 * build file states only what is genuinely specific to it: which Cargo package
 * holds its `cdylib`, and which optional services it uses.
 */
abstract class CranposeExtension @Inject constructor(objects: ObjectFactory) {

    /** The Cargo workspace root, relative to the Gradle project or absolute. */
    val workspaceRoot: Property<String> = objects.property(String::class.java)

    /** The Cargo package whose `cdylib` is packaged into the APK. */
    val cargoPackage: Property<String> = objects.property(String::class.java)

    /**
     * The library name `NativeActivity` loads, without the `lib` prefix or the
     * `.so` suffix. Defaults to the Cargo package name with dashes replaced by
     * underscores, which is what Cargo names the artifact.
     */
    val libraryName: Property<String> = objects.property(String::class.java)

    /** Cargo features enabled for the Android build. */
    val features: ListProperty<String> = objects.listProperty(String::class.java)

    /**
     * Extra Cargo features for individual architectures, on top of [features].
     *
     * Some native dependencies do not build everywhere — a 32-bit architecture
     * an inference or codec backend has no port for, say. Naming the feature
     * per architecture ships that architecture without it rather than dropping
     * the architecture or the feature. Architectures that end up with the same
     * feature set are still built in one `cargo ndk` pass.
     *
     * Debug and release are stated separately for the same reason their ABIs
     * and profiles are: an expensive backend a release ships is not something
     * an edit/deploy loop should pay for on every build.
     */
    val debugAbiFeatures: MapProperty<String, List<String>> = objects.mapProperty()

    /** Extra Cargo features per architecture for release variants. */
    val releaseAbiFeatures: MapProperty<String, List<String>> = objects.mapProperty()

    /** Whether the Android build passes `--no-default-features`. */
    val defaultFeatures: Property<Boolean> = objects.property(Boolean::class.java)

    /** ABIs built for debug variants. */
    val debugAbis: ListProperty<String> = objects.listProperty(String::class.java)

    /** ABIs built for release variants. */
    val releaseAbis: ListProperty<String> = objects.listProperty(String::class.java)

    /**
     * The Cargo profile used for release variants.
     *
     * Defaults to `release`, the one profile Cargo defines for every project.
     * A profile the plugin picked instead would have to be declared in the
     * application's own `Cargo.toml`, and a release build that fails with
     * `profile is not defined` before the application's code is even reached
     * is not something a plugin gets to impose. An application that keeps a
     * faster local release profile names it here and declares it itself.
     */
    val releaseProfile: Property<String> = objects.property(String::class.java)

    /** The Cargo profile used for debug variants. */
    val debugProfile: Property<String> = objects.property(String::class.java)

    /**
     * The Android API level the native library is linked against.
     *
     * Defaults to the application's own `minSdk`, which is the level the APK is
     * supported on anyway. It matters because `cargo-ndk` otherwise links
     * against API 21, whose sysroot has no `libaaudio.so` — an application that
     * enables Cranpose's audio backend then fails to link with `unable to find
     * library -laaudio`, on a build that never mentioned an API level.
     */
    val androidApiLevel: Property<Int> = objects.property(Int::class.java)

    /**
     * Environment variables set for the Cargo build.
     *
     * What a `build.rs` reads: the version Gradle stamped into the package, a
     * build identifier, an API endpoint chosen per flavour. Cargo rebuilds when
     * a variable the build script marked changes, so a value that comes from
     * Gradle reaches the binary rather than the last one that did.
     */
    val environment: MapProperty<String, String> =
        objects.mapProperty(String::class.java, String::class.java)

    /**
     * The optional services this application uses. Each one contributes the
     * permissions and components that service needs and nothing else, so an
     * application that does not record video never asks for the camera.
     *
     * Valid names: `background`, `billing`, `camera`, `haptics`, `media`,
     * `notifications`, `overlay`, `update`.
     */
    val services: SetProperty<String> = objects.setProperty(String::class.java)

    /** The activity label, used for the launcher entry. */
    val label: Property<String> = objects.property(String::class.java)

    /** The activity theme resource. */
    val theme: Property<String> = objects.property(String::class.java)

    /**
     * The Cranpose artifact version. Defaults to the plugin's own version, so
     * the Java, the manifest contributions and the plugin never disagree.
     */
    val cranposeVersion: Property<String> = objects.property(String::class.java)
}
