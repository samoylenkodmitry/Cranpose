// Deterministic benchmark data: `shared-kotlin/dev/perfcompare/shared/PerfData.kt`
// and `cranpose-app/src/data.rs` implement the same generator bit for bit, so
// every app draws identical content. Only what the gauntlet shows is kept.

import 'dart:math' as math;
import 'dart:typed_data';

const int postCount = 5000;
const int barCount = 24;
const int cardRowsPerCluster = 5;
const int avatarCount = 8;
const int avatarSize = 64;
const int sparkPoints = 48;

const _words = [
  'lorem', 'ipsum', 'dolor', 'sit', 'amet', 'consectetur', 'adipiscing', 'elit', 'sed', 'do',
  'eiusmod', 'tempor', 'incididunt', 'ut', 'labore', 'et', 'dolore', 'magna', 'aliqua', 'enim',
  'ad', 'minim', 'veniam', 'quis', 'nostrud', 'exercitation', 'ullamco', 'laboris', 'nisi',
  'aliquip', 'ex', 'ea', 'commodo', 'consequat', 'duis', 'aute', 'irure', 'in', 'reprehenderit',
  'voluptate', 'velit', 'esse', 'cillum', 'fugiat', 'nulla', 'pariatur',
];
const _first = [
  'Ada', 'Linus', 'Grace', 'Alan', 'Barbara', 'Dennis', 'Ken', 'Margaret', 'Edsger', 'Donald',
  'Frances', 'John', 'Radia', 'Tim', 'Guido', 'Bjarne',
];
const _last = [
  'Lovelace', 'Torvalds', 'Hopper', 'Turing', 'Liskov', 'Ritchie', 'Thompson', 'Hamilton',
  'Dijkstra', 'Knuth', 'Allen', 'McCarthy', 'Perlman', 'Berners', 'Rossum', 'Stroustrup',
];
const _tags = [
  '#rust', '#kotlin', '#compose', '#android', '#gpu', '#layout', '#text', '#perf', '#wgpu',
  '#skia', '#ui', '#mobile',
];

const paletteArgb = [
  0xFFEF4444, 0xFFF97316, 0xFFEAB308, 0xFF22C55E, 0xFF14B8A6, 0xFF3B82F6, 0xFF8B5CF6, 0xFFEC4899,
];
const chipBackgroundArgb = [
  0xFFFEE2E2, 0xFFFFEDD5, 0xFFFEF9C3, 0xFFDCFCE7, 0xFFCCFBF1, 0xFFDBEAFE, 0xFFEDE9FE, 0xFFFCE7F3,
];
const gradientEndArgb = [
  0xFF7F1D1D, 0xFF7C2D12, 0xFF713F12, 0xFF14532D, 0xFF134E4A, 0xFF1E3A8A, 0xFF4C1D95, 0xFF831843,
];

/// 32-bit xorshift, in unsigned 32-bit arithmetic as in the other apps.
class Rng {
  int _state;

  Rng(int seed) : _state = ((seed * 0x9E3779B9) & 0xFFFFFFFF) ^ 0xA5A5A5A5 {
    if (_state == 0) _state = 1;
  }

  int next() {
    var x = _state;
    x ^= (x << 13) & 0xFFFFFFFF;
    x ^= x >> 17;
    x ^= (x << 5) & 0xFFFFFFFF;
    _state = x;
    return x;
  }

  int below(int bound) => next() % bound;

  double unit() => (next() >> 8) / 16777216.0;
}

class Post {
  final int id;
  final String author;
  final int color;
  final String title;
  final String body;
  final List<(String, int)> tags;
  final List<String> stats;

  const Post(this.id, this.author, this.color, this.title, this.body, this.tags, this.stats);
}

String _sentence(Rng rng, int min, int max) {
  final count = min + rng.below(max - min + 1);
  final text = List.generate(count, (_) => _words[rng.below(46)]).join(' ');
  return text[0].toUpperCase() + text.substring(1);
}

String compactCount(int value) =>
    value >= 1000 ? '${value ~/ 1000}.${(value % 1000) ~/ 100}k' : '$value';

Post post(int index) {
  final rng = Rng(index + 1);
  final first = _first[rng.below(16)];
  final last = _last[rng.below(16)];
  rng.below(59); // minutes, in the handle the gauntlet does not show
  rng.below(1000); // the handle's number
  final color = rng.below(8);
  final title = _sentence(rng, 4, 8);
  final body = '${_sentence(rng, 22, 38)}.';
  for (var bar = 0; bar < barCount; bar++) {
    rng.unit();
  }
  final tags = List.generate(3, (_) {
    final tag = rng.below(_tags.length);
    return (_tags[tag], tag % 8);
  });
  final stats = List.generate(4, (_) => compactCount(rng.below(20000)));
  return Post(index, '$first $last', color, title, body, tags, stats);
}

List<Post> posts() => List.generate(postCount, post);

class GauntletTier {
  final int columns;
  final double scale;
  final int tickers;
  final int depth;

  const GauntletTier(this.columns, this.scale, this.tickers, this.depth);
}

const gauntletTiers = [
  GauntletTier(1, 1.0, 8, 6),
  GauntletTier(2, 0.85, 12, 8),
  GauntletTier(2, 0.7, 16, 10),
  GauntletTier(3, 0.6, 20, 12),
  GauntletTier(3, 0.5, 28, 14),
  GauntletTier(4, 0.45, 36, 16),
  GauntletTier(4, 0.4, 44, 20),
  GauntletTier(5, 0.35, 56, 24),
];

GauntletTier gauntletTier(int tier) => gauntletTiers[tier.clamp(1, gauntletTiers.length) - 1];

/// A ticker symbol and how its price moves: `Ticker` in the other apps.
class Quote {
  final String symbol;
  final int baseCents;
  final int swingCents;
  final int step;
  final int phase;

  const Quote(this.symbol, this.baseCents, this.swingCents, this.step, this.phase);
}

List<Quote> tickers(int count) {
  final rng = Rng(31337);
  return List.generate(count, (_) {
    final length = 3 + rng.below(2);
    final symbol = String.fromCharCodes(List.generate(length, (_) => 65 + rng.below(26)));
    final base = 1000 + rng.below(99000);
    final swing = 50 + rng.below(950);
    final step = 1 + rng.below(9);
    final phase = rng.below(2000);
    return Quote(symbol, base, swing, step, phase);
  });
}

int _triangle(int value, int period) {
  final position = value % period;
  return period ~/ 2 - (position - period ~/ 2).abs();
}

int tickerCents(Quote ticker, int frame) {
  final wave = _triangle(frame * ticker.step + ticker.phase, 2000);
  return ticker.baseCents + (ticker.swingCents * (wave - 500)) ~/ 500;
}

String _twoDecimals(int hundredths) {
  final value = hundredths.abs();
  final fraction = value % 100;
  return '${value ~/ 100}.${fraction < 10 ? '0' : ''}$fraction';
}

String centsText(int cents) => '${cents < 0 ? '-' : ''}${_twoDecimals(cents)}';

String changeText(Quote ticker, int cents) {
  final basisPoints = ((cents - ticker.baseCents) * 10000) ~/ ticker.baseCents;
  return '${basisPoints < 0 ? '-' : '+'}${_twoDecimals(basisPoints)}%';
}

int progressPermille(int card, int frame) => (frame * 3 + card * 37) % 1000;

double badgeDegrees(int card, int frame) => _triangle(frame * 2 + card * 30, 40) * 0.5 - 5.0;

double widthFraction(int frame) => 0.92 + 0.08 * _triangle(frame * 3, 200) / 100.0;

double sparkValue(int card, int point, int frame) {
  final phase = (frame + card * 7.0) * 0.11;
  return 0.5 + 0.38 * math.sin(point * 0.32 + phase) + 0.08 * math.sin(point * 1.7 + card);
}

const _avatarColors = [
  [0xEF, 0x44, 0x44, 0x7F, 0x1D, 0x1D],
  [0xF9, 0x73, 0x16, 0x7C, 0x2D, 0x12],
  [0xEA, 0xB3, 0x08, 0x71, 0x3F, 0x12],
  [0x22, 0xC5, 0x5E, 0x14, 0x53, 0x2D],
  [0x14, 0xB8, 0xA6, 0x13, 0x4E, 0x4A],
  [0x3B, 0x82, 0xF6, 0x1E, 0x3A, 0x8A],
  [0x8B, 0x5C, 0xF6, 0x4C, 0x1D, 0x95],
  [0xEC, 0x48, 0x99, 0x83, 0x18, 0x43],
];

/// Avatar [index] as RGBA bytes: a radial gradient crossed by diagonal stripes.
Uint8List avatarRgba(int index) {
  final colors = _avatarColors[index % avatarCount];
  const size = avatarSize;
  final pixels = Uint8List(size * size * 4);
  var at = 0;
  for (var y = 0; y < size; y++) {
    for (var x = 0; x < size; x++) {
      final dx = x - size ~/ 2;
      final dy = y - size * 3 ~/ 8;
      final t = math.min(255, (dx * dx + dy * dy) * 255 ~/ (40 * 40));
      final stripe = ((x + y + index * 3) ~/ 6) % 2 == 0;
      for (var channel = 0; channel < 3; channel++) {
        var value = (colors[channel] * (255 - t) + colors[channel + 3] * t) ~/ 255;
        if (stripe) value += (255 - value) ~/ 6;
        pixels[at++] = value;
      }
      pixels[at++] = 0xFF;
    }
  }
  return pixels;
}
