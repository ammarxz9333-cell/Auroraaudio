import 'package:flutter_test/flutter_test.dart';
import 'package:daviewer/features/multisource/reddit_gallery_search.dart';

void main() {
  final adapter = RedditGallerySearch(
    accessToken: () async => null,
    userAgent: 'android:com.example.approvedviewer:0.8 (by /u/example)',
  );

  test('Reddit endpoints use OAuth and cursor pagination', () {
    final uri = adapter.searchUri('fantasy illustration', after: 't3_abc');
    expect(uri.scheme, 'https');
    expect(uri.host, 'oauth.reddit.com');
    expect(uri.path, '/search');
    expect(uri.queryParameters['after'], 't3_abc');
    expect(uri.queryParameters['q'], 'fantasy illustration');
    expect(uri.queryParameters['raw_json'], '1');
  });

  test('Reddit search refuses unauthenticated requests', () async {
    await expectLater(
      adapter.search('illustration'),
      throwsStateError,
    );
  });

  test('Post filter accepts static Reddit-hosted image and galleries only', () {
    final result = adapter.parsePayload({
      'data': {
        'after': 't3_end',
        'children': [
          {
            'data': {
              'id': 'img',
              'permalink': '/r/Art/comments/img/illustration/',
              'title': 'A painting',
              'subreddit_name_prefixed': 'r/Art',
              'post_hint': 'image',
              'url_overridden_by_dest': 'https://i.redd.it/a.jpg',
              'over_18': false,
            }
          },
          {
            'data': {
              'id': 'unsafe',
              'permalink': '/r/Art/comments/unsafe/',
              'post_hint': 'image',
              'url_overridden_by_dest': 'https://i.redd.it/b.jpg',
              'over_18': true,
            }
          },
          {
            'data': {
              'id': 'video',
              'permalink': '/r/Art/comments/video/',
              'url_overridden_by_dest': 'https://v.redd.it/videofile',
              'is_video': true,
            }
          },
          {
            'data': {
              'id': 'external',
              'permalink': '/r/Art/comments/external/',
              'post_hint': 'image',
              'url_overridden_by_dest': 'https://other.example.org/c.jpg',
            }
          },
          {
            'data': {
              'id': 'gallery',
              'permalink': '/r/ImaginaryLandscapes/comments/gallery/',
              'title': 'Clouds',
              'is_gallery': true,
              'media_metadata': {
                'file1': {
                  'm': 'image/jpeg',
                  's': {'u': 'https://preview.redd.it/1.jpg'},
                },
                'file2': {
                  'm': 'image/jpeg',
                  's': {'u': 'https://preview.redd.it/2.jpg'},
                },
                'file3': {
                  'm': 'image/gif',
                  's': {'u': 'https://preview.redd.it/3.gif'},
                },
              },
              'gallery_data': {
                'items': [
                  {'media_id': 'file1'},
                  {'media_id': 'file2'},
                  {'media_id': 'file3'},
                ]
              }
            }
          },
        ]
      }
    });
    expect(result.items.length, 3);
    expect(result.items.map((item) => item.id), [
      'reddit:img',
      'reddit:gallery:file1',
      'reddit:gallery:file2',
    ]);
    expect(result.items.first.creator, 'r/Art');
    expect(result.items.first.pageUrl.host, 'www.reddit.com');
    expect(result.hasMore, isTrue);
  });

  test('Reddit rejects embedded links and spoilers', () {
    final result = adapter.parsePayload({
      'data': {
        'after': null,
        'children': [
          {
            'data': {
              'id': 'spoiler',
              'permalink': '/r/Art/comments/spoiler/',
              'post_hint': 'image',
              'url': 'https://i.redd.it/image.png',
              'spoiler': true,
            }
          },
          {
            'data': {
              'id': 'text',
              'permalink': '/r/Art/comments/text/',
              'is_self': true,
              'url': 'https://i.redd.it/image.png',
            }
          },
        ],
      },
    });
    expect(result.items, isEmpty);
    expect(result.hasMore, isFalse);
  });
}
