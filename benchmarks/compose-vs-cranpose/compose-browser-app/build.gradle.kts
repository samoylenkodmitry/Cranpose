// The gauntlet in Compose Multiplatform for the web: the composables the
// Android app draws, from `shared-compose`, compiled to WebAssembly and drawn
// by Skia on a canvas.
import org.jetbrains.kotlin.gradle.ExperimentalWasmDsl

plugins {
    id("org.jetbrains.kotlin.multiplatform")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.compose")
}

kotlin {
    @OptIn(ExperimentalWasmDsl::class)
    wasmJs {
        outputModuleName.set("perfCompose")
        browser {
            commonWebpackConfig {
                outputFileName = "perf-compose.js"
            }
        }
        binaries.executable()
    }
    sourceSets {
        // One source set: `shared-compose` names the page's `Roboto`,
        // `perfLog` and `avatarBitmap` without declaring them, as it does for
        // the desktop app.
        wasmJsMain {
            kotlin.srcDir("../shared-kotlin")
            kotlin.srcDir("../shared-compose")
            dependencies {
                implementation(compose.runtime)
                implementation(compose.foundation)
                implementation(compose.ui)
            }
        }
    }
}

// The same stability configuration as the Android app.
composeCompiler {
    stabilityConfigurationFiles.add(layout.projectDirectory.file("../compose-app/compose_stability.conf"))
}
