// The standalone Cranpose starter, built through the Cranpose Gradle plugin.
//
// The native build, the ABIs, the Cargo profiles, the JNI packaging, the
// framework's Java and its manifest contributions all come from the plugin.
// What remains is what is specific to this application.
plugins {
    id("com.android.application")
    id("dev.cranpose.android")
}

cranpose {
    // This starter is its own Cargo workspace, two directories above the Gradle
    // project, rather than the framework's.
    workspaceRoot.set("../..")
    cargoPackage.set("isolated-demo")
    label.set("Cranpose Isolated Demo")
}

android {
    namespace = "com.cranpose.isolated.demo"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.cranpose.isolated.demo"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
