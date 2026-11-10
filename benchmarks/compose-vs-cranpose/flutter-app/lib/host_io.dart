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

/// The tier, the freeze frame and the screen: `am start`'s arguments on
/// Android, on a desktop `PERF_TIER`, `PERF_FREEZE` and `PERF_SCENARIO`, which
/// `desktop.py` sets.
(int, int, String) launchOf(List<String> args) {
  int number(String? text, int fallback) => int.tryParse(text ?? '') ?? fallback;
  final environment = Platform.environment;
  if (Platform.isAndroid) {
    return (number(args.elementAtOrNull(0), 5), number(args.elementAtOrNull(1), 0), args.elementAtOrNull(2) ?? 'gauntlet');
  }
  return (
    number(environment['PERF_TIER'], 5),
    number(environment['PERF_FREEZE'], 0),
    environment['PERF_SCENARIO'] ?? 'gauntlet',
  );
}

/// A font file in the folder `PERF_FONTS` names, which every desktop app loads.
Future<ByteData> fontFile(String file) async {
  final folder = Platform.environment['PERF_FONTS'] ?? 'fonts';
  return ByteData.sublistView(await File('$folder/$file').readAsBytes());
}

/// A Roboto file: the device's own on Android, elsewhere the desktop's.
Future<ByteData> robotoFont(String file) async => Platform.isAndroid
    ? ByteData.sublistView(await File('/system/fonts/$file').readAsBytes())
    : fontFile(file);

/// Whether the platform draws the patchwork, whose chat needs a data thread.
bool get drawsPatchwork => Platform.isMacOS || Platform.isWindows || Platform.isLinux;

/// A picture of the patchwork's chat from the folder `PERF_CHAT` names.
Future<Uint8List> chatFile(String file) {
  final folder = Platform.environment['PERF_CHAT'] ?? 'chat-assets';
  return File('$folder/$file').readAsBytes();
}
