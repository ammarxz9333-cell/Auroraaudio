import 'package:flutter_test/flutter_test.dart';
import 'package:daviewer/features/multisource/gallery_catalog.dart';

void main() {
  final catalog = GalleryCatalog();

  test('safe catalog excludes adult source and Reddit OAuth source', () {
    expect(safeGallerySources, isNot(contains(GallerySource.rule34)));
    expect(safeGallerySources, isNot(contains(GallerySource.reddit)));
    expect(safeGallerySources, contains(GallerySource.danbooru));
  });

  test('cannot query adult source without explicit opt-in', () {
    expect(
      () => catalog.buildSearchUri(GallerySource.rule34, 'landscape', 1),
      throwsStateError,
    );
    expect(
      catalog.buildSearchUri(
        GallerySource.rule34,
        'fantasy art',
        1,
        adultEnabled: true,
      ).host,
      'api.rule34.xxx',
    );
  });

  test('cannot use public API path for Reddit', () {
    expect(
      () => catalog.buildSearchUri(GallerySource.reddit, 'illustration', 1),
      throwsStateError,
    );
  });

  test('Danbooru client query restricts to general-rated images', () {
    final uri = catalog.buildSearchUri(
      GallerySource.danbooru,
      'cat girl',
      2,
    );
    expect(uri.host, 'danbooru.donmai.us');
    expect(uri.queryParameters['tags'], contains('rating:g'));
    expect(uri.queryParameters['page'], '2');
  });

  test('Danbooru parser rejects non-general content defensively', () {
    final page = catalog.parsePayload(GallerySource.danbooru, <Object>[
      {
        'id': 1,
        'rating': 'g',
        'tag_string_general': 'artwork sky',
        'file_url': 'https://cdn.donmai.us/original.jpg',
      },
      {
        'id': 2,
        'rating': 'e',
        'file_url': 'https://cdn.donmai.us/explicit.jpg',
      },
      {
        'id': 3,
        'rating': 's',
        'file_url': 'https://cdn.donmai.us/sensitive.jpg',
      },
    ], 1);
    expect(page.items.map((item) => item.id), <String>['danbooru:1']);
  });

  test('Booru image-only filtering excludes video files', () {
    final page = catalog.parsePayload(GallerySource.safebooru, [
      {
        'id': 42,
        'tags': 'landscape',
        'file_url': 'https://safebooru.org/images/42.mp4',
        'preview_url': 'https://safebooru.org/thumbnails/42.jpg',
      },
      {
        'id': 43,
        'tags': 'landscape',
        'file_url': 'https://safebooru.org/images/43.png',
        'preview_url': 'https://safebooru.org/thumbnails/43.jpg',
      },
    ], 1);
    expect(page.items.map((item) => item.id), <String>['safebooru:43']);
  });

  test('Wallhaven excludes unsafe records', () {
    final page = catalog.parsePayload(GallerySource.wallhaven, {
      'meta': {'last_page': 3},
      'data': [
        {
          'id': 'abc',
          'purity': 'sfw',
          'path': 'https://w.wallhaven.cc/full/abc.jpg',
          'thumbs': {'small': 'https://th.wallhaven.cc/small/abc.jpg'},
        },
        {
          'id': 'xyz',
          'purity': 'nsfw',
          'path': 'https://w.wallhaven.cc/full/xyz.jpg',
          'thumbs': {'small': 'https://th.wallhaven.cc/small/xyz.jpg'},
        },
      ],
    }, 1);
    expect(page.items.single.id, 'wallhaven:abc');
    expect(page.hasMore, isTrue);
  });

  test('Openverse excludes marked mature images', () {
    final page = catalog.parsePayload(GallerySource.openverse, {
      'page_count': 1,
      'results': [
        {
          'id': 'a',
          'mature': false,
          'url': 'https://images.example.org/a.jpg',
          'thumbnail': 'https://images.example.org/a-thumb.jpg',
          'foreign_landing_url': 'https://example.org/a',
          'title': 'Landscape',
          'license': 'cc0',
        },
        {
          'id': 'b',
          'mature': true,
          'url': 'https://images.example.org/b.jpg',
          'foreign_landing_url': 'https://example.org/b',
        },
      ],
    }, 1);
    expect(page.items.single.title, 'Landscape');
    expect(page.items.single.license, 'cc0');
    expect(page.hasMore, isFalse);
  });

  test('Only secure HTTPS media URLs are accepted', () {
    final page = catalog.parsePayload(GallerySource.danbooru, [
      {
        'id': 100,
        'rating': 'g',
        'file_url': 'http://unsafe.example/file.jpg',
      }
    ], 1);
    expect(page.items, isEmpty);
  });
}
