// The chat's data layer: an isolate, the thread Dart has, sends one message
// a token and no more tokens than the screen granted, so it waits while the
// screen is behind, as the web page's worker does (`web-app/src/chat-worker.ts`).

import 'dart:collection';
import 'dart:isolate';

import 'chat_data.dart';

/// The tokens the isolate may send ahead of the screen.
const _ahead = chatTokensPerFrame * 4;

void _send(SendPort tokens) {
  final grants = ReceivePort();
  tokens.send(grants.sendPort);
  var count = 0;
  grants.listen((granted) {
    for (var wanted = granted as int; wanted > 0; wanted--) {
      count++;
      tokens.send(chatTokenAfter(count));
    }
  });
}

class ChatFeed {
  final Isolate _isolate;
  final ReceivePort _tokens;
  final _arrived = Queue<ChatToken>();
  SendPort? _grants;

  ChatFeed._(this._isolate, this._tokens) {
    _tokens.listen((message) {
      if (message is SendPort) {
        _grants = message;
        message.send(_ahead);
      } else {
        _arrived.add(message as ChatToken);
      }
    });
  }

  /// Starts the data isolate.
  static Future<ChatFeed> start() async {
    final tokens = ReceivePort();
    return ChatFeed._(await Isolate.spawn(_send, tokens.sendPort), tokens);
  }

  /// The next token, when one has arrived. The screen never waits for one.
  ChatToken? take() => _arrived.isEmpty ? null : _arrived.removeFirst();

  /// Lets the isolate send [taken] more tokens.
  void grant(int taken) => _grants?.send(taken);

  void dispose() {
    _isolate.kill();
    _tokens.close();
  }
}
