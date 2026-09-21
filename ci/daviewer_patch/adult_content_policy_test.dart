import 'package:dakit_core/dakit_core.dart';
import 'package:daviewer/core/content/adult_content_policy.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('accepts mature image with adult semantic signal', () {
    expect(
      isAdultOnlyArtwork(
        _artwork(
          isMature: true,
          tags: const <String>['explicit', 'fantasy'],
        ),
      ),
      isTrue,
    );
  });

  test('rejects mature but non-adult artwork', () {
    expect(
      isAdultOnlyArtwork(
        _artwork(
          isMature: true,
          tags: const <String>['landscape', 'fantasy'],
        ),
      ),
      isFalse,
    );
  });

  test('rejects non-mature artwork even with adult tag', () {
    expect(
      isAdultOnlyArtwork(
        _artwork(
          isMature: false,
          tags: const <String>['explicit'],
        ),
      ),
      isFalse,
    );
  });

  test('age-risk signal always wins over adult signal', () {
    expect(
      isAdultOnlyArtwork(
        _artwork(
          isMature: true,
          tags: const <String>['explicit', 'underage'],
        ),
      ),
      isFalse,
    );
  });

  test('image-only mode rejects media-less and non-image artwork', () {
    expect(
      isAdultOnlyArtwork(
        _artwork(
          isMature: true,
          tags: const <String>['explicit'],
          media: const <MediaAsset>[],
        ),
      ),
      isFalse,
    );
  });

  test('adult page fetch skips filtered source pages', () async {
    var calls = 0;
    final page = await fetchAdultOnlyPage((request) async {
      calls += 1;
      if (calls == 1) {
        return Page<Artwork>(
          items: <Artwork>[
            _artwork(
              id: 'safe',
              isMature: false,
              tags: const <String>['landscape'],
            ),
          ],
          hasMore: true,
          nextCursor: 'next',
        );
      }
      return Page<Artwork>(
        items: <Artwork>[
          _artwork(
            id: 'adult',
            isMature: true,
            tags: const <String>['explicit'],
          ),
        ],
        hasMore: false,
      );
    }, const PageRequest(limit: 24));

    expect(calls, 2);
    expect(page.items.map((item) => item.id), const <String>['adult']);
  });
}

Artwork _artwork({
  String id = 'a',
  required bool isMature,
  required List<String> tags,
  List<MediaAsset>? media,
}) => Artwork(
  id: id,
  title: 'Artwork',
  author: const UserProfile(id: 'u', username: 'artist'),
  pageUri: Uri.parse('https://example.com/art/$id'),
  media: media ??
      <MediaAsset>[
        MediaAsset(
          id: '$id:image',
          kind: MediaKind.image,
          role: MediaRole.preview,
          availability: MediaAvailability.available,
          uri: Uri.parse('https://example.com/$id.jpg'),
        ),
      ],
  isMature: isMature,
  tags: tags,
);
