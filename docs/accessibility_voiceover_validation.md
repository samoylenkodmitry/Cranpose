# Accessibility device validation, 2026-09-22

These checks use CranScan with the local Cranpose changes on top of v0.1.145.
They exercise native user controls without imported database fixtures or app
automation hooks. Receipt images, OCR text, screenshots and speech transcripts
remain in the private audit directory outside the build cache.

## Framework behavior

| Case | Regression evidence | Corrected behavior |
| --- | --- | --- |
| Described images | The shell test reported no image role. | `Image` supplies its role automatically. Explicit modifier semantics retain precedence. Undescribed decorative images remain absent from the reader tree. |
| iPhone receipt navigation | VoiceOver revisited a receipt during forward navigation. | Existing native elements retain reader focus while labels, values and list contents change. The seven-receipt traversal passed in 39.209 seconds. |
| iPhone capture | Actual VoiceOver speech identifies Scan, Take photo and Done before double-tap activation. | The complete library, camera, capture and return flow passed in 61.980 seconds without diagnostic instrumentation. |
| iPhone camera selection | The initial virtual camera had no matching selected lens control. | The active Automatic camera appears in the lens list. Initial selection, Wide selection and return to Automatic passed in 20.962 seconds. Selected radio controls expose the native selected trait. |
| Desktop display density | Logical rectangles failed at 125 percent scale. On the Mac, bottom navigation appeared halfway up the accessibility window. | The AccessKit root transforms logical coordinates once into physical pixels. Unit coverage includes 100, 125, 200 and 300 percent. The physical Mac frame test passed in 2.464 seconds. |

## Scope of the evidence

`just test-reader-actions` passed with 248 shell tests and 383 framework tests on
Linux. The corresponding Mac framework suite passed 384 tests. Both platform
builds include their own platform-specific tests.

The iPhone used iOS 27 and the Mac used macOS 27. The camera-selection and Mac
frame tests validate native accessibility contracts. They do not establish
spoken lens selection or a complete Mac speech flow. The iPhone library and
capture tests use `XCUIVoiceOverService` speech and ordinary VoiceOver activation.

Magic Tap, other screen-reader combinations, application-wide information
parity and every CranScan flow still require separate evidence. A successful
build or native tree snapshot must not be reported as a spoken-navigation pass.
See [the platform validation protocol](accessibility_validation.md).
