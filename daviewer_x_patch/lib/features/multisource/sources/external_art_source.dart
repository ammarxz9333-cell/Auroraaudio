import '../models/external_artwork.dart';

final class ExternalPage {
  const ExternalPage({required this.items, required this.hasMore});

  final List<ExternalArtwork> items;
  final bool hasMore;
}

abstract interface class ExternalArtSource {
  ExternalSourceKind get kind;
  String get label;

  Future<ExternalPage> search(
    String query, {
    int page = 1,
    int limit = 30,
    bool safeMode = true,
  });

  Future<ExternalArtwork?> getById(String id);

  Future<List<ExternalArtwork>> related(
    ExternalArtwork seed, {
    int limit = 24,
    bool safeMode = true,
  });

  Future<List<ExternalArtwork>> artistWorks(
    String artist, {
    int limit = 30,
    bool safeMode = true,
  });
}
