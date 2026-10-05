// The gauntlet in Flutter: the screen `compose-app/.../Gauntlet.kt` and
// `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in
// Flutter's own idiom. The benchmark's README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time, so every framework does the same work per frame.

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import 'data.dart';

const _ink = Color(0xFF111827);
const _body = Color(0xFF374151);
const _muted = Color(0xFF6B7280);
const _hairline = Color(0xFFE5E7EB);
const _panel = Color(0xFFE2E8F0);
const _up = Color(0xFF16A34A);
const _down = Color(0xFFDC2626);
const _white = Color(0xFFFFFFFF);
const _levelBackground = [Color(0xFFF1F5F9), Color(0xFFCBD5E1)];
final _palette = [for (final argb in paletteArgb) Color(argb)];
final _chipBackground = [for (final argb in chipBackgroundArgb) Color(argb)];
final _gradientEnd = [for (final argb in gradientEndArgb) Color(argb)];

/// Blocks of five card rows and a cluster: no measurement window reaches the end.
const _blocks = 200000;

/// How far the list scrolls each frame, in logical pixels (dp).
const _scrollPerFrame = 3.0;

/// The family the device's Roboto files load under, the files the other apps use.
const _roboto = 'PerfRoboto';

/// Carries `PERF` lines to the platform log, under the other apps' tag.
const _log = MethodChannel('perfcompare/log');

/// Writes a `PERF` line: to Android's log, or on a desktop to standard
/// output, where `desktop.py` reads it.
void _perf(String line) {
  if (Platform.isAndroid) {
    _log.invokeMethod<void>('log', line);
  } else {
    stdout.writeln(line);
  }
}

/// What `am start` asked of the gauntlet: tier and freeze frame, as arguments.
class GauntletLoad {
  /// Load tier, 1 to 12.
  final GauntletTier tier;

  /// Stop on this frame and hold still, for picture comparisons; 0 runs on.
  final int freeze;

  const GauntletLoad(this.tier, this.freeze);

  factory GauntletLoad.parse(List<String> args) {
    int arg(int index, int fallback) =>
        index < args.length ? int.tryParse(args[index]) ?? fallback : fallback;
    return GauntletLoad(gauntletTier(arg(0, 5)), arg(1, 0));
  }
}

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  final fonts = FontLoader(_roboto)
    ..addFont(_systemFont('Roboto-Regular.ttf'))
    ..addFont(_systemFont('Roboto-Bold.ttf'));
  final avatars = Future.wait(List.generate(avatarCount, _avatar));
  await fonts.load();
  // On a desktop `desktop.py` asks in `PERF_TIER` and `PERF_FREEZE`.
  final environment = Platform.environment;
  final load = Platform.isAndroid
      ? GauntletLoad.parse(args)
      : GauntletLoad.parse([environment['PERF_TIER'] ?? '', environment['PERF_FREEZE'] ?? '']);
  runApp(GauntletApp(load, posts(), await avatars));
  if (!Platform.isAndroid) {
    WidgetsBinding.instance.addPostFrameCallback((_) => _perf('PERF first_frame'));
  }
}

/// A Roboto file: the device's own on Android, elsewhere the one in the
/// folder `PERF_FONTS` names, which every desktop app loads.
Future<ByteData> _systemFont(String file) async {
  final folder = Platform.isAndroid ? '/system/fonts' : Platform.environment['PERF_FONTS'] ?? 'fonts';
  return ByteData.sublistView(await File('$folder/$file').readAsBytes());
}

Future<ui.Image> _avatar(int index) {
  final image = Completer<ui.Image>();
  ui.decodeImageFromPixels(
      avatarRgba(index), avatarSize, avatarSize, ui.PixelFormat.rgba8888, image.complete);
  return image.future;
}

/// Text of [size] in [color], bold or not, over the root's 1.4 em lines.
TextStyle _text(double size, Color color, {bool bold = false}) => TextStyle(
    fontSize: size, color: color, fontWeight: bold ? FontWeight.bold : FontWeight.normal);

class GauntletApp extends StatelessWidget {
  final GauntletLoad load;
  final List<Post> posts;
  final List<ui.Image> avatars;

  const GauntletApp(this.load, this.posts, this.avatars, {super.key});

  @override
  Widget build(BuildContext context) {
    return Directionality(
      textDirection: TextDirection.ltr,
      // Lines 1.4 em apart without leading above the first or below the last,
      // as Compose's default line height style trims it. Flutter rounds each
      // line to whole logical pixels where Compose rounds it up to whole
      // device pixels, so blocks of text end a few pixels apart.
      child: DefaultTextStyle(
        style: const TextStyle(
            fontFamily: _roboto, height: 1.4, leadingDistribution: TextLeadingDistribution.proportional),
        textHeightBehavior:
            const TextHeightBehavior(applyHeightToFirstAscent: false, applyHeightToLastDescent: false),
        child: ColoredBox(
          color: const Color(0xFFEEF0F5),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Container(
                height: 56,
                color: const Color(0xFF1E2A4A),
                padding: const EdgeInsets.symmetric(horizontal: 16),
                alignment: Alignment.centerLeft,
                child: Text('Gauntlet', style: _text(20, _white, bold: true)),
              ),
              Expanded(child: Gauntlet(load, posts, avatars)),
            ],
          ),
        ),
      ),
    );
  }
}

class Gauntlet extends StatefulWidget {
  final GauntletLoad load;
  final List<Post> posts;
  final List<ui.Image> avatars;

  const Gauntlet(this.load, this.posts, this.avatars, {super.key});

  @override
  State<Gauntlet> createState() => _GauntletState();
}

class _GauntletState extends State<Gauntlet> with SingleTickerProviderStateMixin {
  final _frame = ValueNotifier(0);
  final _scroll = ScrollController();
  late final _ticker = createTicker(_advance);
  late final _quotes = tickers(widget.load.tier.tickers);

  @override
  void initState() {
    super.initState();
    _ticker.start();
  }

  void _advance(Duration elapsed) {
    final index = _frame.value + 1;
    _frame.value = index;
    // The warm-up frame ticks before the list's first layout.
    if (_scroll.hasClients && _scroll.position.hasContentDimensions) {
      _scroll.jumpTo(index * _scrollPerFrame);
    }
    final freeze = widget.load.freeze;
    if (freeze > 0 && index >= freeze) {
      _ticker.stop();
      _perf('PERF frozen frame=$index');
    }
  }

  @override
  void dispose() {
    _ticker.dispose();
    _scroll.dispose();
    _frame.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tier = widget.load.tier;
    final s = tier.scale;
    // The width is read while laying out, not building: the frame lays
    // everything out again without building anything.
    return CustomSingleChildLayout(
      delegate: _WidthFollower(_frame, MediaQuery.devicePixelRatioOf(context)),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ColoredBox(
            color: _panel,
            child: Padding(
              padding: EdgeInsets.all(6 * s),
              child: Wrap(
                spacing: 4 * s,
                runSpacing: 4 * s,
                children: [
                  for (var index = 0; index < _quotes.length; index++)
                    _TickerTile(_quotes[index], index, _frame, s),
                ],
              ),
            ),
          ),
          Expanded(
            child: ListView.separated(
              controller: _scroll,
              padding: EdgeInsets.all(8 * s),
              itemCount: _blocks * (cardRowsPerCluster + 1),
              separatorBuilder: (context, index) => SizedBox(height: 8 * s),
              itemBuilder: (context, row) {
                final block = row ~/ (cardRowsPerCluster + 1);
                final within = row % (cardRowsPerCluster + 1);
                if (within == cardRowsPerCluster) return _Level(block, tier.depth, s);
                final first = (block * cardRowsPerCluster + within) * tier.columns;
                return Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  spacing: 8 * s,
                  children: [
                    for (var card = first; card < first + tier.columns; card++)
                      Expanded(
                        child: _Card(widget.posts[card % postCount],
                            widget.avatars[card % avatarCount], card, _frame, s),
                      ),
                  ],
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}

/// Lays its child out at the frame's share of the width, in whole pixels.
class _WidthFollower extends SingleChildLayoutDelegate {
  final ValueListenable<int> frame;
  final double pixelRatio;

  _WidthFollower(this.frame, this.pixelRatio) : super(relayout: frame);

  @override
  BoxConstraints getConstraintsForChild(BoxConstraints constraints) {
    final pixels = (constraints.maxWidth * pixelRatio * widthFraction(frame.value)).roundToDouble();
    return BoxConstraints.tightFor(width: pixels / pixelRatio, height: constraints.maxHeight);
  }

  @override
  bool shouldRelayout(_WidthFollower oldDelegate) =>
      oldDelegate.frame != frame || oldDelegate.pixelRatio != pixelRatio;
}

/// A rounded track filled to [share] of the frame in [fill].
class _BarPainter extends CustomPainter {
  final ValueListenable<int> frame;
  final double Function(int frame) share;
  final double radius;
  final Paint _track = Paint()..color = _hairline;
  final Paint _fill;

  _BarPainter(this.frame, this.share, this.radius, Color fill)
      : _fill = Paint()..color = fill,
        super(repaint: frame);

  @override
  void paint(Canvas canvas, Size size) {
    final corner = Radius.circular(radius);
    canvas.drawRRect(RRect.fromRectAndRadius(Offset.zero & size, corner), _track);
    final filled = Rect.fromLTWH(0, 0, size.width * share(frame.value), size.height);
    canvas.drawRRect(RRect.fromRectAndRadius(filled, corner), _fill);
  }

  @override
  bool shouldRepaint(_BarPainter oldDelegate) =>
      oldDelegate.frame != frame || oldDelegate.radius != radius || oldDelegate._fill.color != _fill.color;
}

class _TickerTile extends StatelessWidget {
  final Quote quote;
  final int index;
  final ValueListenable<int> frame;
  final double s;

  const _TickerTile(this.quote, this.index, this.frame, this.s);

  @override
  Widget build(BuildContext context) {
    final style = _text(10 * s, _body);
    return DecoratedBox(
      decoration: BoxDecoration(color: _white, borderRadius: BorderRadius.circular(6 * s)),
      child: Padding(
        padding: EdgeInsets.symmetric(horizontal: 6 * s, vertical: 3 * s),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          spacing: 4 * s,
          children: [
            Text(quote.symbol, style: _text(10 * s, _ink, bold: true)),
            // The price and the change rebuild alone each frame.
            ValueListenableBuilder(
              valueListenable: frame,
              builder: (context, frame, _) => Text(centsText(tickerCents(quote, frame)), style: style),
            ),
            ValueListenableBuilder(
              valueListenable: frame,
              builder: (context, frame, _) {
                final cents = tickerCents(quote, frame);
                return Text(changeText(quote, cents),
                    style: style.copyWith(color: cents >= quote.baseCents ? _up : _down));
              },
            ),
            SizedBox(
              width: 20 * s,
              height: 4 * s,
              child: CustomPaint(
                painter: _BarPainter(frame, _share, 2 * s, _palette[index % _palette.length]),
              ),
            ),
          ],
        ),
      ),
    );
  }

  double _share(int frame) =>
      ((tickerCents(quote, frame) - quote.baseCents + quote.swingCents) / (2 * quote.swingCents))
          .clamp(0.0, 1.0);
}

class _Card extends StatelessWidget {
  final Post post;
  final ui.Image avatar;
  final int card;
  final ValueListenable<int> frame;
  final double s;

  const _Card(this.post, this.avatar, this.card, this.frame, this.s);

  @override
  Widget build(BuildContext context) {
    final radius = BorderRadius.circular(12 * s);
    return Stack(
      children: [
        PhysicalModel(
          color: _white,
          elevation: 3 * s,
          shadowColor: const Color(0xFF000000),
          borderRadius: radius,
          child: DecoratedBox(
            decoration: BoxDecoration(border: Border.all(color: _hairline), borderRadius: radius),
            child: Padding(
              padding: EdgeInsets.all(10 * s),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                spacing: 6 * s,
                children: [
                  _header(),
                  Text(post.body, style: _text(12 * s, _body), maxLines: 4, overflow: TextOverflow.ellipsis),
                  _progress(),
                  SizedBox(
                    height: 36 * s,
                    child: CustomPaint(painter: _SparklinePainter(card, _palette[post.color], 1.5 * s, frame)),
                  ),
                  Wrap(
                    spacing: 4 * s,
                    runSpacing: 4 * s,
                    children: [for (final (tag, color) in post.tags) _chip(tag, color)],
                  ),
                  _footer(),
                ],
              ),
            ),
          ),
        ),
        if (card % 5 == 0)
          Positioned(
            top: 0,
            right: 0,
            child: Padding(
              padding: EdgeInsets.all(6 * s),
              // A translucent tag tilting with the frame: transformed again
              // each frame, never built or laid out again.
              child: ValueListenableBuilder(
                valueListenable: frame,
                builder: (context, frame, badge) =>
                    Transform.rotate(angle: badgeDegrees(card, frame) * math.pi / 180, child: badge),
                child: Opacity(
                  opacity: 0.9,
                  child: DecoratedBox(
                    decoration: BoxDecoration(color: _palette[0], borderRadius: BorderRadius.circular(8 * s)),
                    child: Padding(
                      padding: EdgeInsets.symmetric(horizontal: 6 * s, vertical: 2 * s),
                      child: Text('HOT', style: _text(9 * s, _white, bold: true)),
                    ),
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }

  Widget _header() {
    final (tag, color) = post.tags[0];
    return Row(
      spacing: 8 * s,
      children: [
        ClipOval(
          child: RawImage(
              image: avatar, width: 32 * s, height: 32 * s, fit: BoxFit.cover, filterQuality: FilterQuality.low),
        ),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(post.title,
                  style: _text(13 * s, _ink, bold: true), maxLines: 2, overflow: TextOverflow.ellipsis),
              // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
              Text.rich(
                TextSpan(children: [
                  const TextSpan(text: 'by '),
                  TextSpan(text: post.author, style: const TextStyle(fontWeight: FontWeight.bold)),
                  const TextSpan(text: ' · '),
                  TextSpan(text: tag, style: TextStyle(color: _gradientEnd[color])),
                ]),
                style: _text(11 * s, _muted),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _progress() {
    return Row(
      spacing: 6 * s,
      children: [
        Expanded(
          child: SizedBox(
            height: 6 * s,
            child: CustomPaint(
              painter: _BarPainter(frame, _share, 3 * s, _palette[card % _palette.length]),
            ),
          ),
        ),
        ValueListenableBuilder(
          valueListenable: frame,
          builder: (context, frame, _) =>
              Text('${progressPermille(card, frame) ~/ 10}%', style: _text(10 * s, _muted)),
        ),
      ],
    );
  }

  double _share(int frame) => progressPermille(card, frame) / 1000;

  Widget _chip(String tag, int color) {
    return DecoratedBox(
      decoration: BoxDecoration(color: _chipBackground[color], borderRadius: BorderRadius.circular(10 * s)),
      child: Padding(
        padding: EdgeInsets.symmetric(horizontal: 8 * s, vertical: 3 * s),
        child: Text(tag, style: _text(10 * s, _gradientEnd[color])),
      ),
    );
  }

  /// Three counters split by dividers as tall as the tallest counter: an
  /// intrinsic measurement on every pass.
  Widget _footer() {
    const labels = ['likes', 'replies', 'shares'];
    return IntrinsicHeight(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (var index = 0; index < labels.length; index++) ...[
            if (index > 0) const SizedBox(width: 1, child: ColoredBox(color: _hairline)),
            Expanded(
              child: Column(
                children: [
                  Text(post.stats[index], style: _text(12 * s, _ink, bold: true)),
                  Text(labels[index], style: _text(9 * s, _muted)),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// A card's 48-point line over a fading fill, drawn from the frame.
class _SparklinePainter extends CustomPainter {
  final int card;
  final Color color;
  final double strokeWidth;
  final ValueListenable<int> frame;
  final Path _area = Path();
  final Path _line = Path();
  final Paint _fill = Paint();
  final Paint _stroke;
  double _shaderHeight = -1;

  _SparklinePainter(this.card, this.color, this.strokeWidth, this.frame)
      : _stroke = Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = strokeWidth
          ..color = color,
        super(repaint: frame);

  @override
  void paint(Canvas canvas, Size size) {
    if (_shaderHeight != size.height) {
      _shaderHeight = size.height;
      _fill.shader = ui.Gradient.linear(Offset.zero, Offset(0, size.height),
          [color.withValues(alpha: 0.25), color.withValues(alpha: 0)]);
    }
    final step = size.width / (sparkPoints - 1);
    final k = frame.value;
    _area
      ..reset()
      ..moveTo(0, size.height);
    _line.reset();
    for (var index = 0; index < sparkPoints; index++) {
      final x = index * step;
      final y = size.height * (1 - sparkValue(card, index, k));
      _area.lineTo(x, y);
      if (index == 0) {
        _line.moveTo(x, y);
      } else {
        _line.lineTo(x, y);
      }
    }
    _area
      ..lineTo(size.width, size.height)
      ..close();
    canvas
      ..drawPath(_area, _fill)
      ..drawPath(_line, _stroke);
  }

  @override
  bool shouldRepaint(_SparklinePainter oldDelegate) =>
      oldDelegate.card != card ||
      oldDelegate.color != color ||
      oldDelegate.strokeWidth != strokeWidth ||
      oldDelegate.frame != frame;
}

/// `depth` levels nested inside one another, each a row of three labels above
/// the next level.
class _Level extends StatelessWidget {
  final int cluster;
  final int remaining;
  final double s;

  const _Level(this.cluster, this.remaining, this.s);

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
          color: _levelBackground[remaining % 2], borderRadius: BorderRadius.circular(4 * s)),
      child: Padding(
        padding: EdgeInsets.fromLTRB(3 * s, s, s, s),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: s,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              spacing: 2 * s,
              children: [
                for (var chip = 0; chip < 3; chip++)
                  Expanded(
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        color: _palette[(remaining + chip + cluster) % _palette.length],
                        borderRadius: BorderRadius.circular(2 * s),
                      ),
                      child: Text('C$cluster.L$remaining.$chip', style: _text(9 * s, _white)),
                    ),
                  ),
              ],
            ),
            if (remaining > 0) _Level(cluster, remaining - 1, s),
          ],
        ),
      ),
    );
  }
}
