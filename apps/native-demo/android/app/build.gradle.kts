plugins { id("com.android.application") }

android {
    namespace = "dev.cranpose.nativehost"
    compileSdk = 37
    defaultConfig {
        applicationId = "dev.cranpose.nativehost"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    sourceSets["main"].kotlin.srcDir("../../generated/kotlin/uniffi/cranpose_native_demo")
    sourceSets["main"].jniLibs.srcDir("../../generated/jniLibs")
    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":cranpose"))
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
}
