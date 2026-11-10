import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// A page logs `PERF first_frame` itself, as a desktop app does.
bool get logsFirstFrame => true;

/// Writes a `PERF` line to the console, where the page's shim hears it.
void perf(String line) => debugPrint(line);

/// The tier, the freeze frame and the screen from the page's query string:
/// `?tier=12&freeze=120&scenario=wall`.
(int, int, String) launchOf(List<String> args) {
  final query = Uri.base.queryParameters;
  return (
    int.tryParse(query['tier'] ?? '') ?? 5,
    int.tryParse(query['freeze'] ?? '') ?? 0,
    query['scenario'] ?? 'gauntlet',
  );
}

/// A Roboto file from the page's assets: the build copies the files every
/// app loads into `assets/fonts`.
Future<ByteData> robotoFont(String file) => fontFile(file);

/// A page does not draw the patchwork: its chat needs a data thread.
bool get drawsPatchwork => false;

/// The patchwork's chat pictures, which a page never asks for.
Future<Uint8List> chatFile(String file) => throw UnsupportedError('The patchwork runs on the desktop.');

/// A font file from the page's assets.
Future<ByteData> fontFile(String file) => rootBundle.load('assets/fonts/$file');
