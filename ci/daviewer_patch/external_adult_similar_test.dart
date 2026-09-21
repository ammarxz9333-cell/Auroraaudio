import 'package:daviewer/features/artwork/external_adult_similar.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('builds a compact query from meaningful seed tags', () {
    expect(
      buildExternalAdultSearchTags(
        const <String>['digitalart', 'Red Hair', 'Fantasy', 'red hair', 'NSFW'],
      ),
      const <String>['red_hair', 'fantasy'],
    );
  });

  test('strict filter rejects non-explicit and age-risk tags', () {
    final safe = _post(
      rating: 'explicit',
      tags: const <String>{'fantasy', 'red_hair', 'woman'},
    );
    final nonExplicit = _post(
      rating: 'questionable',
      tags: const <String>{'fantasy', 'red_hair'},
    );
    final blocked = _post(
      rating: 'e',
      tags: const <String>{'fantasy', 'loli'},
    );

    expect(isStrictAdultExternalPost(safe), isTrue);
    expect(isStrictAdultExternalPost(nonExplicit), isFalse);
    expect(isStrictAdultExternalPost(blocked), isFalse);
  });

  test('parses Rule34-style JSON list and filters blocked posts', () {
    final posts = parseExternalAdultPosts(
      ExternalAdultSource.rule34,
      <Object?>[
        <String, Object?>{
          'id': 1,
          'rating': 'e',
          'tags': 'fantasy red_hair woman',
          'preview_url': 'https://cdn.example/1.jpg',
          'file_url': 'https://cdn.example/1-full.jpg',
          'width': 1000,
          'height': 1500,
        },
        <String, Object?>{
          'id': 2,
          'rating': 'e',
          'tags': 'fantasy shota',
          'preview_url': 'https://cdn.example/2.jpg',
          'file_url': 'https://cdn.example/2-full.jpg',
        },
      ],
    );

    expect(posts, hasLength(1));
    expect(posts.single.id, '1');
  });

  test('parses Gelbooru-style post wrapper', () {
    final posts = parseExternalAdultPosts(
      ExternalAdultSource.gelbooru,
      <String, Object?>{
        '@attributes': <String, Object?>{'count': 1},
        'post': <Object?>[
          <String, Object?>{
            'id': '55',
            'rating': 'explicit',
            'tags': 'fantasy red_hair',
            'preview_url': 'https://cdn.example/55.jpg',
            'file_url': 'https://cdn.example/55-full.jpg',
          },
        ],
      },
    );

    expect(posts, hasLength(1));
    expect(posts.single.source, ExternalAdultSource.gelbooru);
  });

  test('ranking prefers shared tags and similar aspect ratio', () {
    final close = _post(
      rating: 'e',
      tags: const <String>{'fantasy', 'red_hair', 'woman'},
      width: 1000,
      height: 1500,
    );
    final far = _post(
      rating: 'e',
      tags: const <String>{'landscape'},
      width: 2000,
      height: 500,
    );

    final closeScore = externalSimilarityScore(
      seedTags: const <String>{'fantasy', 'red_hair'},
      candidate: close,
      seedAspectRatio: 2 / 3,
    );
    final farScore = externalSimilarityScore(
      seedTags: const <String>{'fantasy', 'red_hair'},
      candidate: far,
      seedAspectRatio: 2 / 3,
    );

    expect(closeScore, greaterThan(farScore));
  });
}

ExternalAdultPost _post({
  required String rating,
  required Set<String> tags,
  int? width,
  int? height,
}) => ExternalAdultPost(
  source: ExternalAdultSource.rule34,
  id: '1',
  pageUri: Uri.parse('https://example.com/post/1'),
  previewUri: Uri.parse('https://example.com/preview.jpg'),
  fileUri: Uri.parse('https://example.com/file.jpg'),
  tags: tags,
  rating: rating,
  width: width,
  height: height,
);
