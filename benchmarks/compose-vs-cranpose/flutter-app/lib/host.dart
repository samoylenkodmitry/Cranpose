// What the gauntlet asks of the platform it runs on, which `dart:io` answers on
// Android and the desktop and a page answers in a browser, where `dart:io` does
// not exist: where its `PERF` lines go, what it was launched with, and where
// its Roboto comes from.
export 'host_io.dart' if (dart.library.js_interop) 'host_web.dart';
