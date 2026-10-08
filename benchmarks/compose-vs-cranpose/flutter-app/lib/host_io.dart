import 'dart:io';

import 'package:flutter/services.dart';

/// Carries `PERF` lines to the platform log, under the other apps' tag.
const _log = MethodChannel('perfcompare/log');

/// Whether the app logs `PERF first_frame` itself: Android's launch code logs
/// it on the native side.
bool get logsFirstFrame => !Platform.isAndroid;

/// Writes a `PERF` line: to Android's log, or on a desktop to standard
/// output, where `desktop.py` reads it.
void perf(String line) {
  if (Platform.isAndroid) {
    _log.invokeMethod<void>('log', line);
  } else {
    stdout.writeln(line);
  }
}

/// The tier and the freeze frame: `am start`'s arguments on Android, on a
/// desktop `PERF_TIER` and `PERF_FREEZE`, which `desktop.py` sets.
(int, int) launchOf(List<String> args) {
  int number(String? text, int fallback) => int.tryParse(text ?? '') ?? fallback;
  final environment = Platform.environment;
  if (Platform.isAndroid) {
    return (number(args.elementAtOrNull(0), 5), number(args.elementAtOrNull(1), 0));
  }
  return (number(environment['PERF_TIER'], 5), number(environment['PERF_FREEZE'], 0));
}

/// A Roboto file: the device's own on Android, elsewhere the one in the
/// folder `PERF_FONTS` names, which every desktop app loads.
Future<ByteData> robotoFont(String file) async {
  final folder = Platform.isAndroid ? '/system/fonts' : Platform.environment['PERF_FONTS'] ?? 'fonts';
  return ByteData.sublistView(await File('$folder/$file').readAsBytes());
}
