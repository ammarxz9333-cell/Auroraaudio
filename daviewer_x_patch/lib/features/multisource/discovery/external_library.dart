import 'dart:convert';
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

import '../models/external_artwork.dart';

final class ExternalLibraryState {
  const ExternalLibraryState({
    this.saved = const <String, ExternalArtwork>{},
    this.followedArtists = const <String>{},
    this.ready = false,
  });

  final Map<String, ExternalArtwork> saved;
  final Set<String> followedArtists;
  final bool ready;

  ExternalLibraryState copyWith({
    Map<String, ExternalArtwork>? saved,
    Set<String>? followedArtists,
    bool? ready,
  }) => ExternalLibraryState(
        saved: saved ?? this.saved,
        followedArtists: followedArtists ?? this.followedArtists,
        ready: ready ?? this.ready,
      );
}

final externalLibraryProvider =
    StateNotifierProvider<ExternalLibraryController, ExternalLibraryState>((ref) {
  return ExternalLibraryController();
});

final class ExternalLibraryController extends StateNotifier<ExternalLibraryState> {
  ExternalLibraryController() : super(const ExternalLibraryState()) {
    _load();
  }

  static const _filename = 'daviewer_x_library.json';

  bool isSaved(String key) => state.saved.containsKey(key);

  bool isFollowing(ExternalSourceKind source, String artist) =>
      state.followedArtists.contains(_artistKey(source, artist));

  Future<void> toggleSaved(ExternalArtwork artwork) async {
    final next = Map<String, ExternalArtwork>.from(state.saved);
    if (next.containsKey(artwork.key)) {
      next.remove(artwork.key);
    } else {
      next[artwork.key] = artwork;
    }
    state = state.copyWith(saved: Map.unmodifiable(next));
    await _persist();
  }

  Future<void> toggleFollow(ExternalSourceKind source, String artist) async {
    final next = Set<String>.from(state.followedArtists);
    final key = _artistKey(source, artist);
    if (!next.add(key)) next.remove(key);
    state = state.copyWith(followedArtists: Set.unmodifiable(next));
    await _persist();
  }

  Future<void> _load() async {
    try {
      final file = await _file();
      if (!await file.exists()) {
        state = state.copyWith(ready: true);
        return;
      }
      final raw = jsonDecode(await file.readAsString());
      if (raw is! Map<String, dynamic>) throw const FormatException('library');
      final saved = <String, ExternalArtwork>{};
      final items = raw['saved'];
      if (items is List) {
        for (final item in items.whereType<Map<String, dynamic>>()) {
          final parsed = ExternalArtwork.fromJson(item.cast<String, Object?>());
          if (parsed != null) saved[parsed.key] = parsed;
        }
      }
      final follows = (raw['followedArtists'] as List<dynamic>? ?? const <dynamic>[])
          .map((e) => e.toString())
          .toSet();
      state = ExternalLibraryState(
        saved: Map.unmodifiable(saved),
        followedArtists: Set.unmodifiable(follows),
        ready: true,
      );
    } on Object {
      state = state.copyWith(ready: true);
    }
  }

  Future<void> _persist() async {
    final file = await _file();
    await file.writeAsString(
      const JsonEncoder.withIndent('  ').convert(<String, Object?>{
        'saved': state.saved.values.map((e) => e.toJson()).toList(),
        'followedArtists': state.followedArtists.toList()..sort(),
      }),
      flush: true,
    );
  }

  Future<File> _file() async {
    final dir = await getApplicationSupportDirectory();
    return File('${dir.path}${Platform.pathSeparator}$_filename');
  }

  static String _artistKey(ExternalSourceKind source, String artist) =>
      '${source.name}|$artist';
}
