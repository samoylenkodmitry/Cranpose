// The status line's spinner, drawn from the paths of `chat-assets/progress.svg`:
// the file's `<path>` elements hold absolute `M`, `C` and `Z` commands, which
// `Spinner` parses once and paints with a painter that never repaints, so a
// turn of the spinner is a transform of its layer.

import 'package:flutter/widgets.dart';

/// One stroked path of the SVG.
class _SpinnerPath {
  final Path path;
  final Paint paint;

  _SpinnerPath(String data, this.paint) : path = _parse(data);
}

final _attributes = RegExp(r'([\w-]+)="([^"]*)"');
final _elements = RegExp(r'<path\s+([^>]*?)/>');
final _commands = RegExp(r'([MCZ])([^MCZ]*)');
final _number = RegExp(r'-?\d+(?:\.\d+)?');

Path _parse(String data) {
  final path = Path();
  for (final command in _commands.allMatches(data)) {
    final at = [for (final number in _number.allMatches(command[2]!)) double.parse(number[0]!)];
    switch (command[1]) {
      case 'M':
        path.moveTo(at[0], at[1]);
      case 'C':
        for (var curve = 0; curve < at.length; curve += 6) {
          path.cubicTo(at[curve], at[curve + 1], at[curve + 2], at[curve + 3], at[curve + 4], at[curve + 5]);
        }
      case 'Z':
        path.close();
    }
  }
  return path;
}

class _SpinnerPainter extends CustomPainter {
  final double viewBox;
  final List<_SpinnerPath> paths;

  _SpinnerPainter(this.viewBox, this.paths);

  @override
  void paint(Canvas canvas, Size size) {
    canvas.scale(size.width / viewBox);
    for (final stroked in paths) {
      canvas.drawPath(stroked.path, stroked.paint);
    }
  }

  @override
  bool shouldRepaint(_SpinnerPainter oldDelegate) => false;
}

/// The SVG text's spinner, [size] points on a side.
class Spinner extends StatelessWidget {
  final String svg;
  final double size;

  const Spinner(this.svg, this.size, {super.key});

  @override
  Widget build(BuildContext context) {
    final viewBox = double.parse(RegExp(r'viewBox="0 0 (\d+)').firstMatch(svg)![1]!);
    final paths = <_SpinnerPath>[];
    for (final element in _elements.allMatches(svg)) {
      final attributes = {for (final match in _attributes.allMatches(element[1]!)) match[1]!: match[2]!};
      final paint = Paint()
        ..style = PaintingStyle.stroke
        ..color = Color(0xFF000000 | int.parse(attributes['stroke']!.substring(1), radix: 16))
        ..strokeWidth = double.parse(attributes['stroke-width']!)
        ..strokeCap = attributes['stroke-linecap'] == 'round' ? StrokeCap.round : StrokeCap.butt;
      paths.add(_SpinnerPath(attributes['d']!, paint));
    }
    return SizedBox(
      width: size,
      height: size,
      child: RepaintBoundary(child: CustomPaint(painter: _SpinnerPainter(viewBox, paths))),
    );
  }
}
