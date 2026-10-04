import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:dio/dio.dart';

import '../models/external_artwork.dart';

final class VisualSimilarityReranker {
  VisualSimilarityReranker(
    this._dio, {
    this.pixivBridgeUrl = '',
    this.pixivBridgeToken = '',
  });

  final Dio _dio;
  final String pixivBridgeUrl;
  final String pixivBridgeToken;

  Future<List<ExternalArtwork>> rerank(
    ExternalArtwork seed,
    List<ExternalArtwork> candidates, {
    int maxCandidates = 36,
  }) async {
    if (candidates.length < 2) return candidates;
    final seedHash = await _hash(seed.previewUri);
    if (seedHash == null) return candidates;
    final head = candidates.take(maxCandidates).toList(growable: false);
    final tail = candidates.skip(maxCandidates).toList(growable: false);
    final scored = <_Scored>[];
    const batchSize = 6;
    for (var start = 0; start < head.length; start += batchSize) {
      final end = (start + batchSize < head.length) ? start + batchSize : head.length;
      final batch = head.sublist(start, end);
      final hashes = await Future.wait(batch.map((item) => _hash(item.previewUri)));
      for (var i = 0; i < batch.length; i++) {
        final hash = hashes[i];
        scored.add(_Scored(batch[i], hash == null ? 999 : _hamming(seedHash, hash), start + i));
      }
    }
    scored.sort((a, b) {
      final byDistance = a.distance.compareTo(b.distance);
      return byDistance != 0 ? byDistance : a.originalIndex.compareTo(b.originalIndex);
    });
    return <ExternalArtwork>[...scored.map((e) => e.artwork), ...tail];
  }

  Future<int?> _hash(Uri uri) async {
    try {
      final response = await _dio.get<List<int>>(
        uri.toString(),
        options: Options(
          responseType: ResponseType.bytes,
          receiveTimeout: const Duration(seconds: 12),
          headers: _headersFor(uri),
        ),
      );
      final bytes = response.data;
      if (bytes == null || bytes.isEmpty) return null;
      final codec = await ui.instantiateImageCodec(
        Uint8List.fromList(bytes),
        targetWidth: 9,
        targetHeight: 8,
      );
      try {
        final frame = await codec.getNextFrame();
        try {
          final data = await frame.image.toByteData(format: ui.ImageByteFormat.rawRgba);
          if (data == null) return null;
          return _dHash(data.buffer.asUint8List(), frame.image.width, frame.image.height);
        } finally {
          frame.image.dispose();
        }
      } finally {
        codec.dispose();
      }
    } on Object {
      return null;
    }
  }

  Map<String, String>? _headersFor(Uri uri) {
    if (pixivBridgeToken.isEmpty || pixivBridgeUrl.isEmpty) return null;
    if (uri.toString().startsWith(pixivBridgeUrl)) {
      return <String, String>{'X-Bridge-Token': pixivBridgeToken};
    }
    return null;
  }

  static int _dHash(Uint8List rgba, int width, int height) {
    if (width < 9 || height < 8) return 0;
    var hash = 0;
    var bit = 0;
    for (var y = 0; y < 8; y++) {
      for (var x = 0; x < 8; x++) {
        final left = _luma(rgba, width, x, y);
        final right = _luma(rgba, width, x + 1, y);
        if (left > right) hash |= (1 << bit);
        bit += 1;
      }
    }
    return hash;
  }

  static int _luma(Uint8List rgba, int width, int x, int y) {
    final offset = ((y * width) + x) * 4;
    final r = rgba[offset];
    final g = rgba[offset + 1];
    final b = rgba[offset + 2];
    return (r * 299 + g * 587 + b * 114) ~/ 1000;
  }

  static int _hamming(int a, int b) {
    var value = a ^ b;
    var count = 0;
    while (value != 0) {
      value &= value - 1;
      count += 1;
    }
    return count;
  }
}

final class _Scored {
  const _Scored(this.artwork, this.distance, this.originalIndex);
  final ExternalArtwork artwork;
  final int distance;
  final int originalIndex;
}
