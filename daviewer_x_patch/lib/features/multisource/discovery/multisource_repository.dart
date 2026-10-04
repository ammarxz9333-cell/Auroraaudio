import '../models/external_artwork.dart';
import '../sources/external_art_source.dart';
import 'visual_similarity.dart';

final class ArtistSuggestion {
  const ArtistSuggestion({
    required this.source,
    required this.artist,
    required this.score,
    required this.sample,
  });

  final ExternalSourceKind source;
  final String artist;
  final int score;
  final ExternalArtwork sample;
}

final class MultiSourceRepository {
  MultiSourceRepository(
    Iterable<ExternalArtSource> sources, {
    VisualSimilarityReranker? visualReranker,
  })  : _visualReranker = visualReranker,
        _sources = <ExternalSourceKind, ExternalArtSource>{
          for (final source in sources) source.kind: source,
        };

  final Map<ExternalSourceKind, ExternalArtSource> _sources;
  final VisualSimilarityReranker? _visualReranker;

  List<ExternalSourceKind> get availableSources =>
      List<ExternalSourceKind>.unmodifiable(_sources.keys);

  ExternalArtSource? source(ExternalSourceKind kind) => _sources[kind];

  Future<List<ExternalArtwork>> searchAcross(
    String query, {
    ExternalSourceKind? only,
    int page = 1,
    int perSource = 24,
    bool safeMode = true,
  }) async {
    final sources = only == null
        ? _sources.values.toList(growable: false)
        : <ExternalArtSource>[if (_sources[only] case final source?) source];
    final results = await Future.wait(
      sources.map((source) async {
        try {
          return await source.search(query, page: page, limit: perSource, safeMode: safeMode);
        } on Object {
          return const ExternalPage(items: <ExternalArtwork>[], hasMore: false);
        }
      }),
    );
    return _roundRobin(results.map((e) => e.items).toList());
  }

  Future<ExternalArtwork?> getById(ExternalSourceKind source, String id) =>
      _sources[source]?.getById(id) ?? Future<ExternalArtwork?>.value();

  Future<List<ExternalArtwork>> relatedAcross(
    ExternalArtwork seed, {
    int perSource = 16,
    bool safeMode = true,
  }) async {
    final seedTags = seed.tags.take(4).join(' ');
    final futures = <Future<List<ExternalArtwork>>>[];
    for (final source in _sources.values) {
      futures.add(() async {
        try {
          if (source.kind == seed.source) {
            return await source.related(seed, limit: perSource, safeMode: safeMode);
          }
          final page = await source.search(seedTags, limit: perSource, safeMode: safeMode);
          return page.items;
        } on Object {
          return const <ExternalArtwork>[];
        }
      }());
    }
    final lists = await Future.wait(futures);
    final seen = <String>{seed.key};
    final candidates = _roundRobin(lists)
        .where((item) => seen.add(item.key))
        .toList(growable: false);
    final reranker = _visualReranker;
    if (reranker == null || candidates.length < 2) return candidates;
    return reranker.rerank(seed, candidates);
  }

  Future<List<ArtistSuggestion>> suggestArtists(
    ExternalArtwork seed, {
    int limit = 12,
    bool safeMode = true,
  }) async {
    final related = await relatedAcross(seed, safeMode: safeMode);
    final byArtist = <String, _ArtistAccumulator>{};
    for (final item in related) {
      final artist = item.artist.trim();
      if (artist.isEmpty || artist == 'unknown') continue;
      if (item.source == seed.source && artist == seed.artist) continue;
      final key = '${item.source.name}|$artist';
      byArtist.update(
        key,
        (value) => value..score += 1,
        ifAbsent: () => _ArtistAccumulator(item, 1),
      );
    }
    final result = byArtist.entries
        .map((entry) => ArtistSuggestion(
              source: entry.value.sample.source,
              artist: entry.value.sample.artist,
              score: entry.value.score,
              sample: entry.value.sample,
            ))
        .toList()
      ..sort((a, b) => b.score.compareTo(a.score));
    return result.take(limit).toList(growable: false);
  }

  Future<List<ExternalArtwork>> followingFeed(
    Iterable<String> followedArtistKeys, {
    int perArtist = 12,
    bool safeMode = true,
  }) async {
    final jobs = <Future<List<ExternalArtwork>>>[];
    for (final key in followedArtistKeys) {
      final split = key.split('|');
      if (split.length < 2) continue;
      final kind = ExternalSourceKind.values.where((e) => e.name == split.first).firstOrNull;
      final source = kind == null ? null : _sources[kind];
      if (source == null) continue;
      final artist = split.sublist(1).join('|');
      jobs.add(() async {
        try {
          return await source.artistWorks(artist, limit: perArtist, safeMode: safeMode);
        } on Object {
          return const <ExternalArtwork>[];
        }
      }());
    }
    final lists = await Future.wait(jobs);
    final items = lists.expand((e) => e).toList();
    items.sort((a, b) {
      final ad = a.publishedAt;
      final bd = b.publishedAt;
      if (ad == null && bd == null) return 0;
      if (ad == null) return 1;
      if (bd == null) return -1;
      return bd.compareTo(ad);
    });
    return items;
  }

  static List<ExternalArtwork> _roundRobin(List<List<ExternalArtwork>> lists) {
    final output = <ExternalArtwork>[];
    var index = 0;
    while (true) {
      var added = false;
      for (final list in lists) {
        if (index < list.length) {
          output.add(list[index]);
          added = true;
        }
      }
      if (!added) break;
      index += 1;
    }
    return output;
  }
}

final class _ArtistAccumulator {
  _ArtistAccumulator(this.sample, this.score);
  final ExternalArtwork sample;
  int score;
}

extension _FirstOrNull<T> on Iterable<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
