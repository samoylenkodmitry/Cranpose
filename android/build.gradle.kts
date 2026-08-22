// The Cranpose Android distribution: the framework's Java, its manifest
// contributions, and the Gradle plugin that configures a consuming application.
//
// `./gradlew publishToMavenLocal` from this directory publishes every artifact
// under the `dev.cranpose` group, which is what a consumer resolves.
allprojects {
    group = "dev.cranpose"
    version = providers
        .fileContents(rootProject.layout.projectDirectory.file("../Cargo.toml"))
        .asText
        .map { text ->
            Regex("""^version\s*=\s*"([^"]+)"""", RegexOption.MULTILINE)
                .find(text)
                ?.groupValues
                ?.get(1)
                ?: error("the workspace Cargo.toml must declare a version")
        }
        .get()
}
