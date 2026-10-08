import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// A page logs `PERF first_frame` itself, as a desktop app does.
bool get logsFirstFrame => true;

/// Writes a `PERF` line to the console, where the page's shim hears it.
void perf(String line) => debugPrint(line);

/// The tier and the freeze frame from the page's query string:
/// `?tier=12&freeze=120`.
(int, int) launchOf(List<String> args) {
  final query = Uri.base.queryParameters;
  return (int.tryParse(query['tier'] ?? '') ?? 5, int.tryParse(query['freeze'] ?? '') ?? 0);
}

/// A Roboto file from the page's assets: the build copies the files every
/// app loads into `assets/fonts`.
Future<ByteData> robotoFont(String file) => rootBundle.load('assets/fonts/$file');
