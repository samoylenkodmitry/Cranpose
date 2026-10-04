# Cranpose services

Service contracts for Cranpose apps across platforms. The crate
groups APIs for HTTP, files, URI handlers, haptics, audio and media, device and
power information, accessibility, app updates, purchases, navigation and
incoming shares. Applications access composition-scoped services through
their `local_*` functions and `Provide*` composables, or call the plain
functions for platform-wide state.

Platform backends register through the setters in each service module. Optional
Cargo features select implementations; `cranpose` enables the appropriate
service features for its platform. The service traits and their fallback
implementations stay available across targets, and callers can check
reported capabilities before they request platform behavior.

`cranpose-audio` supplies short sound effects and `cranpose-media` supplies
long-form playback on Android and desktop. The `audio` and `media` features on
`cranpose` control those implementations. Android applications configure the
Gradle plugin's service list for required components and declare their
permissions through `cranpose-capabilities`.
