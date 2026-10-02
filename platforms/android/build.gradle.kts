plugins { id("com.android.library") }

extensions.configure<com.android.build.api.dsl.LibraryExtension> {
    namespace = "dev.cranpose"
    compileSdk = 37
    defaultConfig { minSdk = 24 }
    sourceSets["main"].kotlin.srcDir(rootProject.file(providers.gradleProperty("cranposeBindingsDir").get()))
    sourceSets["main"].java.srcDir("../../crates/cranpose/android/java-webview")
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
}
