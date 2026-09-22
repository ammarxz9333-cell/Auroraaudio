import 'dart:convert';
import 'dart:io';

import 'package:dakit_api/dakit_api.dart';
import 'package:dakit_core/dakit_core.dart';
import 'package:dio/dio.dart';
import 'package:test/test.dart';

void main() {
  test('maps the authenticated user and ignores additive fields', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      await fixture('whoami.json'),
    ]);

    final user = await OfficialAccountRepository(transport).currentUser();

    expect(user.id, 'user-1');
    expect(user.username, 'sample-user');
    expect(user.avatarUri?.host, 'images.example.test');
    expect(transport.requests.single.path, 'user/whoami');
  });

  test('maps artwork image and video variants without treating previews as originals', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      await fixture('deviation.json'),
    ]);

    final artwork = await OfficialArtworkRepository(transport).getById('art-1');

    expect(artwork.title, 'Example work');
    expect(artwork.publishedAt, DateTime.utc(2026, 8, 20, 3, 4, 5));
    expect(artwork.isDownloadable, isTrue);
    expect(artwork.downloadAvailability, MediaAvailability.available);
    expect(artwork.textContent?.format, 'writer');
    expect(artwork.textContent?.features, 'tables,links');
    expect(artwork.media, hasLength(3));
    expect(
      artwork.media.map((asset) => asset.kind),
      containsAll(<MediaKind>[MediaKind.image, MediaKind.video]),
    );
    expect(
      artwork.media.every((asset) => asset.role == MediaRole.preview),
      isTrue,
    );
    expect(transport.requests.single.path, 'deviation/art-1');
    expect(transport.requests.single.query['expand'], 'deviation.fulltext');
  });

  test('maps formatted_excerpt when excerpt is absent', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'deviationid': 'art-1',
        'title': 'T',
        'url': 'https://www.deviantart.com/u/art/t-1',
        'author': <String, Object?>{'userid': 'u-1', 'username': 'u'},
        'formatted_excerpt': 'Formatted description.',
      },
    ]);

    final artwork = await OfficialArtworkRepository(transport).getById('art-1');
    expect(artwork.description, 'Formatted description.');
  });

  test('loads omitted artwork tags from deviation metadata', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'metadata': <Object?>[
          <String, Object?>{
            'deviationid': 'other',
            'tags': <Object?>[
              <String, Object?>{'tag_name': 'ignored'},
            ],
          },
          <String, Object?>{
            'deviationid': 'art-1',
            'tags': <Object?>[
              <String, Object?>{'tag_name': 'portrait'},
              <String, Object?>{'tag_name': 'digitalart'},
              <String, Object?>{'tag_name': 'portrait'},
            ],
          },
        ],
      },
    ]);

    final tags = await OfficialArtworkMetadataRepository(transport)
        .tags('art-1');

    expect(tags, const <String>['portrait', 'digitalart']);
    expect(transport.requests.single.path, 'deviation/metadata');
    expect(transport.requests.single.query, <String, Object?>{
      'deviationids[]': <String>['art-1'],
      'mature_content': true,
    });
  });

  test(
    'loads rendered literature without executing provider HTML or CSS',
    () async {
      final transport = FixtureTransport(<Map<String, Object?>>[
        await fixture('content.json'),
      ]);

      final content = await OfficialArtworkContentRepository(transport)
          .get('art-1');

      expect(content.artworkId, 'art-1');
      expect(content.html, contains('Rendered literature'));
      expect(content.cssFonts, hasLength(1));
      expect(content.isEmpty, isFalse);
      expect(transport.requests.single.path, 'deviation/content');
      expect(transport.requests.single.query, <String, Object?>{
        'deviationid': 'art-1',
        'for_edit': false,
        'mature_content': true,
      });
    },
  );

  test(
    'distinguishes paid and blocked original access in detail metadata',
    () async {
      final paidJson = await fixture('deviation.json')
        ..['is_downloadable'] = false
        ..['premium_folder_data'] = <String, Object?>{
          'type': 'premium',
          'has_access': false,
          'gallery_id': 'paid-gallery',
        };
      final blockedJson = await fixture('deviation.json')
        ..['is_downloadable'] = false
        ..['is_blocked'] = true;
      final transport = FixtureTransport(<Map<String, Object?>>[
        paidJson,
        blockedJson,
      ]);
      final repository = OfficialArtworkRepository(transport);

      final paid = await repository.getById('paid');
      final blocked = await repository.getById('blocked');

      expect(paid.downloadAvailability, MediaAvailability.purchaseRequired);
      expect(blocked.downloadAvailability, MediaAvailability.restricted);

      // The full-size `content` image is gated alongside the download, but the
      // `preview` thumbnail stays available so hosts can still show a preview.
      expect(
        paid.media.any(
          (asset) =>
              asset.kind == MediaKind.image &&
              asset.availability == MediaAvailability.purchaseRequired,
        ),
        isTrue,
      );
      expect(
        paid.media.any(
          (asset) =>
              asset.kind == MediaKind.image &&
              asset.availability == MediaAvailability.available,
        ),
        isTrue,
      );
    },
  );

  test('uses documented home pagination and search parameters', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      await fixture('gallery_page.json'),
      await fixture('gallery_page.json'),
    ]);
    final repository = OfficialArtworkRepository(transport);

    final first = await repository.browse(const PageRequest(limit: 20));
    final search = await repository.search(
      'landscape',
      const PageRequest(cursor: '24', limit: 12),
    );

    expect(first.hasMore, isTrue);
    expect(first.nextCursor, '24');
    expect(search.items.single.id, 'art-1');
    expect(transport.requests[0].query, <String, Object?>{
      'offset': 0,
      'limit': 20,
    });
    expect(transport.requests[1].query, <String, Object?>{
      'offset': 24,
      'limit': 12,
      'q': 'landscape',
    });
  });

  test('requests more-like-this preview and maps related deviations and collections', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      await fixture('more_like_this_preview.json'),
    ]);
    final repository = OfficialDiscoveryRepository(transport);

    final result = await repository.moreLikeThis('art-1');

    expect(transport.requests.single.path, 'browse/morelikethis/preview');
    expect(transport.requests.single.query, <String, Object?>{
      'seed': 'art-1',
      'mature_content': true,
    });
    // "More from DA" first, then "More from artist", de-duplicated and without
    // the seed itself.
    expect(result.artworks.map((artwork) => artwork.id), <String>[
      'art-3',
      'art-2',
    ]);
    expect(
      result.artworks.singleWhere((artwork) => artwork.id == 'art-3').media,
      isNotEmpty,
    );
    // Collection groups keep their owner and the deviations inside them.
    final featured = result.featuredInCollections.single;
    expect(featured.collection.folderId, 111);
    expect(featured.collection.name, 'Curated picks');
    expect(featured.collection.owner.username, 'curator');
    expect(
      featured.collection.coverUri,
      Uri.parse('https://images.example.test/cover.jpg'),
    );
    expect(featured.deviations.single.id, 'art-4');
    expect(result.suggestedCollections.single.collection.folderId, 222);
    expect(result.suggestedCollections.single.deviations.single.id, 'art-5');
  });

  test('degrades to empty collections when their shape drifts', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'seed': 'art-1',
        'author': <String, Object?>{'userid': 'u', 'username': 'u'},
        'more_from_artist': <Object?>[],
        'more_from_da': <Object?>[
          <String, Object?>{
            'deviationid': 'art-3',
            'title': 'T',
            'url': 'https://www.deviantart.com/u/art/t-3',
            'author': <String, Object?>{'userid': 'u', 'username': 'u'},
            'preview': <String, Object?>{
              'src': 'https://images.example.test/3.jpg',
            },
          },
        ],
        'featured_in_collections': <String, Object?>{'not': 'a list'},
        'suggested_collections': 'garbage',
      },
    ]);

    final result = await OfficialDiscoveryRepository(transport)
        .moreLikeThis('art-1');

    // The related artwork still renders even though the collection groups are
    // malformed (graceful degradation instead of a hard parse failure).
    expect(result.artworks.single.id, 'art-3');
    expect(result.featuredInCollections, isEmpty);
    expect(result.suggestedCollections, isEmpty);
  });

  test(
    'hydrates sparse more-like-this entries allowed by the schema',
    () async {
      final hydrated = await fixture('deviation.json')
        ..['deviationid'] = 'sparse-art';
      final transport = FixtureTransport(<Map<String, Object?>>[
        <String, Object?>{
          'seed': 'art-1',
          'author': <String, Object?>{'userid': 'u', 'username': 'u'},
          'more_from_da': <Object?>[
            <String, Object?>{
              'deviationid': 'sparse-art',
              'printid': null,
              'is_deleted': false,
            },
          ],
          'more_from_artist': <Object?>[],
        },
        hydrated,
      ]);

      final result = await OfficialDiscoveryRepository(transport)
          .moreLikeThis('art-1');

      expect(result.artworks.single.id, 'sparse-art');
      expect(result.artworks.single.media, isNotEmpty);
      expect(transport.requests, hasLength(2));
      expect(transport.requests.last.path, 'deviation/sparse-art');
      expect(transport.requests.last.query, <String, Object?>{
        'mature_content': true,
        'expand': 'deviation.fulltext',
      });
    },
  );

  test('surfaces sparse hydration failure when no item remains', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'seed': 'art-1',
        'author': <String, Object?>{'userid': 'u', 'username': 'u'},
        'more_from_da': <Object?>[
          <String, Object?>{
            'deviationid': 'still-sparse',
            'printid': null,
            'is_deleted': false,
          },
        ],
        'more_from_artist': <Object?>[],
      },
      <String, Object?>{
        'deviationid': 'still-sparse',
        'printid': null,
        'is_deleted': false,
      },
    ]);

    await expectLater(
      OfficialDiscoveryRepository(transport).moreLikeThis('art-1'),
      throwsA(isA<DAKitException>()),
    );
  });

  test(
    'skips malformed related entries without dropping valid siblings',
    () async {
      final transport = FixtureTransport(<Map<String, Object?>>[
        <String, Object?>{
          'more_from_artist': <Object?>[
            <String, Object?>{'deviationid': 'deleted-without-required-fields'},
            <String, Object?>{
              'deviationid': 'art-2',
              'title': 'Still available',
              'url': 'https://www.deviantart.com/u/art/still-2',
              'author': <String, Object?>{'userid': 'u', 'username': 'u'},
              'preview': <String, Object?>{
                'src': 'https://images.example.test/2.jpg',
              },
            },
          ],
          // The provider may omit one rail entirely; the other remains useful.
          'featured_in_collections': <Object?>[],
          'suggested_collections': <Object?>[],
        },
      ]);

      final result = await OfficialDiscoveryRepository(transport)
          .moreLikeThis('art-1');

      expect(result.artworks.map((artwork) => artwork.id), <String>['art-2']);
    },
  );

  test('clamps gallery limit to the provider maximum of 24', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      await fixture('gallery_page.json'),
    ]);

    await OfficialGalleryRepository(transport)
        .gallery('sample-user', const PageRequest(limit: 50));

    expect(transport.requests.single.path, 'gallery/all');
    expect(transport.requests.single.query['limit'], 24);
  });

  test('loads a full user profile through a stable domain model', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'user': <String, Object?>{
          'userid': 'user-1',
          'username': 'sample-user',
          'usericon': 'https://images.example.test/avatar.png',
        },
        'is_watching': true,
        'profile_url': 'https://www.deviantart.com/sample-user',
        'user_is_artist': true,
        'artist_level': 'Professional',
        'artist_specialty': null,
        'real_name': 'Sample Artist',
        'tagline': 'Making examples',
        'countryid': 1,
        'country': 'Exampleland',
        'website': '',
        'bio': 'Profile biography',
        'cover_photo': 'https://images.example.test/cover.jpg',
        'last_status': null,
        'stats': <String, Object?>{
          'user_deviations': 12,
          'user_favourites': 34,
          'user_comments': 56,
          'profile_pageviews': 78,
          'profile_comments': 9,
        },
        'future_field': true,
      },
    ]);

    final profile = await OfficialUserRepository(transport)
        .profile('sample-user');

    expect(profile.user.username, 'sample-user');
    expect(profile.isWatching, isTrue);
    expect(profile.stats.deviations, 12);
    expect(profile.website, isNull);
    expect(profile.coverPhotoUri?.host, 'images.example.test');
    expect(transport.requests.single.path, 'user/profile/sample-user');
    expect(
      transport.requests.single.query['expand'],
      'user.details,user.geo,user.stats',
    );
  });

  test('loads user relationships, watch state, and friend search', () async {
    final friend = relationshipJson('friend-1', watchesYou: true);
    final watcher = relationshipJson('watcher-1');
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'results': <Object?>[friend],
        'has_more': true,
        'next_offset': 10,
      },
      <String, Object?>{
        'results': <Object?>[watcher],
        'has_more': false,
        'next_offset': null,
      },
      <String, Object?>{'watching': true},
      <String, Object?>{
        'results': <Object?>[
          <String, Object?>{'userid': 'match-1', 'username': 'matching-user'},
        ],
      },
    ]);
    final repository = OfficialUserRepository(transport);

    final friends = await repository.friends(
      'sample-user',
      const PageRequest(limit: 10),
    );
    final watchers = await repository.watchers(
      'sample-user',
      const PageRequest(limit: 10),
    );
    final watching = await repository.isWatching('friend-1');
    final matches = await repository.searchFriends(
      'matching',
      username: 'sample-user',
    );

    expect(friends.items.single.watchesYou, isTrue);
    expect(friends.items.single.watchOptions.deviations, isTrue);
    expect(friends.nextCursor, '10');
    expect(watchers.items.single.watchesYou, isNull);
    expect(watching, isTrue);
    expect(matches.single.username, 'matching-user');
    expect(transport.requests[0].path, 'user/friends/sample-user');
    expect(transport.requests[1].path, 'user/watchers/sample-user');
    expect(transport.requests[2].path, 'user/friends/watching/friend-1');
    expect(transport.requests[3].query['query'], 'matching');
  });

  test('looks up multiple users with one encoded mutation', () async {
    final transport = FixtureMutationTransport(
      postResponses: <Map<String, Object?>>[
        <String, Object?>{
          'results': <Object?>[
            <String, Object?>{'userid': 'user-1', 'username': 'First'},
            <String, Object?>{'userid': 'user-2', 'username': 'Second'},
          ],
        },
      ],
    );

    final users = await OfficialUserLookupRepository(transport)
        .lookup(const <String>['First', 'first', 'Second']);

    expect(users.map((user) => user.username), <String>['First', 'Second']);
    expect(transport.postRequests.single.path, 'user/whois');
    expect(transport.postRequests.single.form['usernames'], <String>[
      'First',
      'Second',
    ]);
  });

  test('loads daily, watched, and cursor-based tag discovery feeds', () async {
    final artwork = await fixture('deviation.json');
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'results': <Object?>[artwork],
      },
      <String, Object?>{
        'results': <Object?>[artwork],
        'has_more': true,
        'next_offset': 20,
      },
      <String, Object?>{
        'results': <Object?>[artwork],
        'has_more': true,
        'next_offset': 10,
        'next_cursor': 'opaque-token',
      },
      <String, Object?>{
        'results': <Object?>[artwork],
        'has_more': false,
        'next_offset': null,
      },
    ]);
    final repository = OfficialDiscoveryRepository(transport);

    final daily = await repository.dailyDeviations(
      date: DateTime(2026, 8, 20, 23, 59),
    );
    final watched = await repository.watched(const PageRequest(limit: 20));
    final firstTag = await repository.tag(
      ' digital-art ',
      const PageRequest(limit: 10),
    );
    await repository.tag(
      'digital-art',
      PageRequest(cursor: firstTag.nextCursor, limit: 10),
    );

    expect(daily.single.id, 'art-1');
    expect(watched.nextCursor, '20');
    expect(firstTag.nextCursor, 'cursor:opaque-token');
    expect(transport.requests[0].query['date'], '2026-08-20');
    expect(transport.requests[1].path, 'browse/deviantsyouwatch');
    expect(transport.requests[2].query['offset'], 0);
    expect(transport.requests[3].query['cursor'], 'opaque-token');
    expect(transport.requests[3].query, isNot(contains('offset')));
  });

  test('treats a missing watched results list as an empty feed', () async {
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{},
      <String, Object?>{'results': null, 'has_more': false},
    ]);
    final repository = OfficialDiscoveryRepository(transport);

    final missing = await repository.watched(const PageRequest(limit: 20));
    final nullResults = await repository.watched(const PageRequest(limit: 20));

    expect(missing.items, isEmpty);
    expect(missing.hasMore, isFalse);
    expect(missing.nextCursor, isNull);
    expect(nullResults.items, isEmpty);
    expect(nullResults.hasMore, isFalse);
    expect(nullResults.nextCursor, isNull);
  });

  test('rejects missing watched results with contradictory pagination', () {
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{'has_more': true, 'next_offset': 20},
    ]);

    expect(
      OfficialDiscoveryRepository(transport)
          .watched(const PageRequest(limit: 20)),
      throwsA(
        isA<DAKitException>().having(
          (error) => error.code,
          'code',
          'api.page.missing_results',
        ),
      ),
    );
  });

  test('falls back to a tagged offset when tag cursor is omitted', () async {
    final artwork = await fixture('deviation.json');
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'results': <Object?>[artwork],
        'has_more': true,
        'next_offset': 10,
        'next_cursor': '',
      },
      <String, Object?>{
        'results': <Object?>[],
        'has_more': false,
        'next_offset': null,
      },
    ]);
    final repository = OfficialDiscoveryRepository(transport);

    final first = await repository.tag('drawing', const PageRequest(limit: 10));
    await repository.tag(
      'drawing',
      PageRequest(cursor: first.nextCursor, limit: 10),
    );

    expect(first.nextCursor, 'offset:10');
    expect(transport.requests[1].query['offset'], 10);
    expect(transport.requests[1].query, isNot(contains('cursor')));
  });

  test('maps topic navigation and tag suggestions', () async {
    final artwork = await fixture('deviation.json');
    final transport = FixtureTransport(<Map<String, Object?>>[
      <String, Object?>{
        'results': <Object?>[
          <String, Object?>{'tag_name': 'digital-art'},
          <String, Object?>{'tag_name': 'digital-painting'},
        ],
      },
      <String, Object?>{
        'results': <Object?>[
          <String, Object?>{
            'name': 'Digital Art',
            'canonical_name': 'digital-art',
            'example_deviations': <Object?>[artwork],
          },
        ],
        'has_more': true,
        'next_offset': 10,
        'next_cursor': 'topics-token',
      },
      <String, Object?>{
        'results': <Object?>[
          <String, Object?>{
            'name': 'Photography',
            'canonical_name': 'photography',
            'example_deviations': artwork,
          },
        ],
      },
      <String, Object?>{
        'results': <Object?>[artwork],
        'has_more': false,
        'next_offset': null,
        'next_cursor': null,
        'prev_cursor': null,
      },
    ]);
    final repository = OfficialDiscoveryRepository(transport);

    final suggestions = await repository.suggestTags(' digital ');
    final topics = await repository.topics(const PageRequest(limit: 10));
    final topTopics = await repository.topTopics();
    final topic = await repository.topic(
      'digital-art',
      const PageRequest(limit: 24),
    );

    expect(suggestions, <String>['digital-art', 'digital-painting']);
    expect(topics.items.single.canonicalName, 'digital-art');
    expect(topics.items.single.exampleArtworks.single.id, 'art-1');
    expect(topics.nextCursor, 'cursor:topics-token');
    expect(topTopics.single.exampleArtworks.single.id, 'art-1');
    expect(topic.items.single.id, 'art-1');
    expect(transport.requests[0].query['tag_name'], 'digital');
    expect(transport.requests[1].path, 'browse/topics');
    expect(transport.requests[3].query['topic'], 'digital-art');
  });

  test(
    'maps gallery and collection folders with optional preload data',
    () async {
      final artwork = await fixture('deviation.json');
      final transport = FixtureTransport(<Map<String, Object?>>[
        <String, Object?>{
          'results': <Object?>[
            <String, Object?>{
              'folderid': 'gallery-1',
              'parent': null,
              'name': 'Featured',
              'description': 'Gallery description',
              'size': 2,
              'thumb': artwork,
              'has_subfolders': true,
              'deviations': <Object?>[artwork],
            },
          ],
          'has_more': false,
          'next_offset': null,
        },
        <String, Object?>{
          'results': <Object?>[
            <String, Object?>{
              'folderid': 'collection-1',
              'name': 'Inspiration',
              'description': '',
              'thumb': null,
            },
          ],
          'has_more': false,
          'next_offset': null,
        },
      ]);
      final repository = OfficialFolderRepository(transport);

      final galleries = await repository.galleryFolders(
        username: 'sample-user',
        options: const FolderQueryOptions(
          calculateSize: true,
          preloadArtworks: true,
        ),
      );
      final collections = await repository.collectionFolders();

      expect(galleries.items.single.kind, FolderKind.gallery);
      expect(galleries.items.single.thumbnail?.id, 'art-1');
      expect(galleries.items.single.preloadedArtworks.single.id, 'art-1');
      expect(galleries.items.single.hasSubfolders, isTrue);
      expect(collections.items.single.kind, FolderKind.collection);
      expect(collections.items.single.thumbnail, isNull);
      expect(transport.requests[0].query['calculate_size'], isTrue);
      expect(transport.requests[0].query['ext_preload'], isTrue);
      expect(transport.requests[1].query, isNot(contains('username')));
    },
  );

  test('loads gallery and collection folder contents', () async {
    final artwork = await fixture('deviation.json');
    final response = <String, Object?>{
      'results': <Object?>[artwork],
      'has_more': false,
      'next_offset': null,
    };
    final transport = FixtureTransport(<Map<String, Object?>>[
      response,
      Map<String, Object?>.of(response),
    ]);
    final repository = OfficialFolderRepository(transport);

    final gallery = await repository.galleryContents(
      'gallery-1',
      username: 'sample-user',
      request: const PageRequest(limit: 24),
    );
    final collection = await repository.collectionContents('collection-1');

    expect(gallery.items.single.id, 'art-1');
    expect(collection.items.single.id, 'art-1');
    expect(transport.requests[0].path, 'gallery/gallery-1');
    expect(transport.requests[0].query['username'], 'sample-user');
    expect(transport.requests[1].path, 'collections/collection-1');
    expect(transport.requests[1].query, isNot(contains('username')));
  });

  test(
    'resolves original transfer metadata through the download endpoint',
    () async {
      final transport = FixtureTransport(<Map<String, Object?>>[
        await fixture('download.json'),
      ]);

      final asset = await OfficialMediaRepository(transport)
          .originalFile('art-1');

      expect(asset.role, MediaRole.original);
      expect(asset.kind, MediaKind.archive);
      expect(asset.byteLength, 50331648);
      expect(asset.canTransfer, isTrue);
      expect(transport.requests.single.path, 'deviation/download/art-1');
    },
  );

  test(
    'maps expected download denials without inventing an original URL',
    () async {
      final cases = <(DAKitException, MediaAvailability)>[
        (
          const DAKitException(
            kind: DAKitFailureKind.upstream,
            code: 'api.provider.invalid_request',
            message: 'Deviation not downloadable.',
            details: <String, Object?>{'provider_code': 2},
          ),
          MediaAvailability.unavailable,
        ),
        (
          const DAKitException(
            kind: DAKitFailureKind.upstream,
            code: 'api.http.400',
            message: 'Download not available.',
            details: <String, Object?>{'status': 400},
          ),
          MediaAvailability.unavailable,
        ),
        (
          const DAKitException(
            kind: DAKitFailureKind.authorization,
            code: 'api.http.403',
            message: 'Restricted.',
          ),
          MediaAvailability.restricted,
        ),
        (
          const DAKitException(
            kind: DAKitFailureKind.authentication,
            code: 'api.http.401',
            message: 'Login required.',
          ),
          MediaAvailability.loginRequired,
        ),
      ];

      for (final (failure, availability) in cases) {
        final asset = await OfficialMediaRepository(ThrowingTransport(failure))
            .originalFile('art-1');
        expect(asset.availability, availability);
        expect(asset.uri, isNull);
        expect(asset.canTransfer, isFalse);
      }
    },
  );

  test(
    'carries the provider description through as availabilityReason',
    () async {
      const failure = DAKitException(
        kind: DAKitFailureKind.upstream,
        code: 'api.provider.invalid_request',
        message: 'Deviation not downloadable.',
        details: <String, Object?>{
          'provider_code': 2,
          'provider_description': 'Free download limit reached.',
        },
      );

      final asset = await OfficialMediaRepository(
        const ThrowingTransport(failure),
      ).originalFile('art-1');

      expect(asset.availability, MediaAvailability.unavailable);
      expect(asset.availabilityReason, 'Free download limit reached.');
      expect(asset.uri, isNull);
    },
  );

  test('keeps unexpected transfer failures observable', () async {
    const failure = DAKitException(
      kind: DAKitFailureKind.network,
      code: 'network.connection',
      message: 'Offline.',
    );

    expect(
      () =>
          OfficialMediaRepository(const ThrowingTransport(failure))
              .originalFile('art-1'),
      throwsA(same(failure)),
    );
  });

  test('keeps 5xx transfer failures observable for a retry', () async {
    const failure = DAKitException(
      kind: DAKitFailureKind.upstream,
      code: 'api.http.500',
      message: 'Server error.',
      details: <String, Object?>{'status': 500},
    );

    expect(
      () =>
          OfficialMediaRepository(const ThrowingTransport(failure))
              .originalFile('art-1'),
      throwsA(same(failure)),
    );
  });

  test(
    'reports a typed parsing failure when required fields disappear',
    () async {
      final transport = FixtureTransport(<Map<String, Object?>>[
        <String, Object?>{'deviationid': 'art-1', 'is_deleted': false},
      ]);

      expect(
        () => OfficialArtworkRepository(transport).getById('art-1'),
        throwsA(
          isA<DAKitException>().having(
            (error) => error.code,
            'code',
            'api.dto.missing_field',
          ),
        ),
      );
    },
  );

  test('loads and posts artwork comments through stable models', () async {
    final transport = FixtureMutationTransport(
      getResponses: <Map<String, Object?>>[
        <String, Object?>{
          'has_more': true,
          'next_offset': 10,
          'has_less': false,
          'prev_offset': null,
          'total': 11,
          'thread': <Object?>[commentJson('comment-1', 'Existing comment')],
        },
      ],
      postResponses: <Map<String, Object?>>[
        commentJson('comment-2', 'Reply text', parentId: 'comment-1'),
      ],
    );
    final repository = OfficialCommentRepository(transport);

    final page = await repository.forArtwork(
      'art-1',
      request: const CommentPageRequest(limit: 10, maxDepth: 2),
    );
    final posted = await repository.postToArtwork(
      'art-1',
      'Reply text',
      parentCommentId: 'comment-1',
    );

    expect(page.items.single.body, 'Existing comment');
    expect(page.nextOffset, 10);
    expect(page.total, 11);
    expect(posted.parentId, 'comment-1');
    expect(transport.getRequests.single.path, 'comments/deviation/art-1');
    expect(transport.getRequests.single.query['maxdepth'], 2);
    expect(transport.postRequests.single.path, 'comments/post/deviation/art-1');
    expect(transport.postRequests.single.form, <String, Object?>{
      'body': 'Reply text',
      'commentid': 'comment-1',
    });
  });

  test(
    'maps favourite and watch mutations without leaking transport types',
    () async {
      final transport = FixtureMutationTransport(
        getResponses: <Map<String, Object?>>[
          <String, Object?>{'success': true},
        ],
        postResponses: <Map<String, Object?>>[
          <String, Object?>{'success': true, 'favourites': 8},
          <String, Object?>{'success': true, 'favourites': 7},
          <String, Object?>{'success': true},
        ],
      );
      final repository = OfficialSocialRepository(transport);

      final added = await repository.favourite(
        'art-1',
        collectionFolderIds: const <String>['folder-1'],
      );
      final removed = await repository.unfavourite('art-1');
      await repository.watch(
        'sample-user',
        options: const WatchOptions(activity: false),
      );
      await repository.unwatch('sample-user');

      expect(added.isFavourite, isTrue);
      expect(added.total, 8);
      expect(removed.isFavourite, isFalse);
      expect(removed.total, 7);
      expect(transport.postRequests[0].path, 'collections/fave');
      expect(transport.postRequests[0].form['folderid'], <String>['folder-1']);
      expect(transport.postRequests[2].form['watch[activity]'], isFalse);
      expect(
        transport.getRequests.single.path,
        'user/friends/unwatch/sample-user',
      );
    },
  );

  test('loads and deletes provider message-center items', () async {
    final artwork = await fixture('deviation.json');
    final message = <String, Object?>{
      'messageid': 'message-1',
      'type': 'feedback',
      'orphaned': false,
      'ts': '2026-08-20T12:00:00Z',
      'stackid': 'stack-1',
      'stack_count': 3,
      'is_new': true,
      'originator': <String, Object?>{
        'userid': 'user-2',
        'username': 'originator',
      },
      'subject': <String, Object?>{'deviation': artwork},
      'html': '<strong>New feedback</strong>',
    };
    Map<String, Object?> offsetPage() => <String, Object?>{
      'results': <Object?>[message],
      'has_more': false,
      'next_offset': null,
    };
    final transport = FixtureMutationTransport(
      getResponses: <Map<String, Object?>>[
        <String, Object?>{
          'results': <Object?>[message],
          'has_more': true,
          'cursor': 'message-cursor',
        },
        offsetPage(),
        offsetPage(),
        offsetPage(),
        offsetPage(),
      ],
      postResponses: <Map<String, Object?>>[
        <String, Object?>{'success': true},
      ],
    );
    final repository = OfficialMessageRepository(transport);

    final feed = await repository.feed(stacked: false);
    await repository.feedback(
      FeedbackType.comments,
      const PageRequest(limit: 10),
    );
    await repository.mentions(const PageRequest(limit: 10));
    await repository.feedbackStack('stack-1', const PageRequest(limit: 10));
    await repository.mentionStack('stack-1', const PageRequest(limit: 10));
    await repository.delete(messageId: 'message-1');

    expect(feed.items.single.type, 'feedback');
    expect(feed.items.single.isNew, isTrue);
    expect(feed.items.single.artwork?.id, 'art-1');
    expect(feed.nextCursor, 'message-cursor');
    expect(transport.getRequests[0].query['stack'], isFalse);
    expect(transport.getRequests[1].query['type'], 'comments');
    expect(transport.getRequests[3].path, 'messages/feedback/stack-1');
    expect(transport.getRequests[4].path, 'messages/mentions/stack-1');
    expect(transport.postRequests.single.form['messageid'], 'message-1');
  });

  test('rejects an ambiguous message delete without a request', () async {
    final transport = FixtureMutationTransport();

    await expectLater(
      OfficialMessageRepository(transport)
          .delete(messageId: 'message-1', stackId: 'stack-1'),
      throwsA(
        isA<DAKitException>().having(
          (error) => error.code,
          'code',
          'api.message.delete.invalid_target',
        ),
      ),
    );
    expect(transport.postRequests, isEmpty);
  });

  test('validates comment limits before making a request', () async {
    final transport = FixtureMutationTransport();

    await expectLater(
      OfficialCommentRepository(transport)
          .forArtwork('art-1', request: const CommentPageRequest(maxDepth: 6)),
      throwsA(
        isA<DAKitException>().having(
          (error) => error.code,
          'code',
          'api.comment.page.invalid',
        ),
      ),
    );
    expect(transport.getRequests, isEmpty);
  });
}

Map<String, Object?> commentJson(String id, String body, {String? parentId}) =>
    <String, Object?>{
      'commentid': id,
      'parentid': parentId,
      'posted': '2026-08-20T12:00:00Z',
      'replies': 0,
      'hidden': null,
      'body': body,
      'is_liked': false,
      'is_featured': false,
      'likes': 2,
      'user': <String, Object?>{'userid': 'user-1', 'username': 'sample-user'},
    };

Map<String, Object?> relationshipJson(String username, {bool? watchesYou}) =>
    <String, Object?>{
      'user': <String, Object?>{'userid': '$username-id', 'username': username},
      'is_watching': true,
      'watches_you': ?watchesYou,
      'lastvisit': '2026-08-20T12:00:00Z',
      'watch': <String, Object?>{
        'friend': true,
        'deviations': true,
        'journals': true,
        'forum_threads': false,
        'critiques': true,
        'scraps': true,
        'activity': true,
        'collections': true,
      },
    };

Future<Map<String, Object?>> fixture(String name) async {
  final packageLocal = File('test/fixtures/$name');
  final workspaceLocal = File('packages/dakit_api/test/fixtures/$name');
  final file = await packageLocal.exists() ? packageLocal : workspaceLocal;
  final data = jsonDecode(await file.readAsString());
  return (data as Map).map<String, Object?>(
    (key, value) => MapEntry(key.toString(), value),
  );
}

final class FixtureTransport implements OfficialApiTransport {
  FixtureTransport(this.responses);

  final List<Map<String, Object?>> responses;
  final List<FixtureRequest> requests = <FixtureRequest>[];

  @override
  Future<Map<String, Object?>> getJson(
    String path, {
    Map<String, Object?> query = const <String, Object?>{},
    CancelToken? cancelToken,
  }) async {
    requests.add(FixtureRequest(path, Map<String, Object?>.of(query)));
    return responses.removeAt(0);
  }
}

final class FixtureRequest {
  const FixtureRequest(this.path, this.query);

  final String path;
  final Map<String, Object?> query;
}

final class FixtureMutationTransport implements OfficialApiMutationTransport {
  FixtureMutationTransport({
    List<Map<String, Object?>> getResponses = const <Map<String, Object?>>[],
    List<Map<String, Object?>> postResponses = const <Map<String, Object?>>[],
  }) : getResponses = List<Map<String, Object?>>.of(getResponses),
       postResponses = List<Map<String, Object?>>.of(postResponses);

  final List<Map<String, Object?>> getResponses;
  final List<Map<String, Object?>> postResponses;
  final List<FixtureRequest> getRequests = <FixtureRequest>[];
  final List<FixtureMutationRequest> postRequests = <FixtureMutationRequest>[];

  @override
  Future<Map<String, Object?>> getJson(
    String path, {
    Map<String, Object?> query = const <String, Object?>{},
    CancelToken? cancelToken,
  }) async {
    getRequests.add(FixtureRequest(path, Map<String, Object?>.of(query)));
    return getResponses.removeAt(0);
  }

  @override
  Future<Map<String, Object?>> postFormJson(
    String path, {
    Map<String, Object?> form = const <String, Object?>{},
    CancelToken? cancelToken,
  }) async {
    postRequests.add(
      FixtureMutationRequest(path, Map<String, Object?>.of(form)),
    );
    return postResponses.removeAt(0);
  }
}

final class FixtureMutationRequest {
  const FixtureMutationRequest(this.path, this.form);

  final String path;
  final Map<String, Object?> form;
}

final class ThrowingTransport implements OfficialApiTransport {
  const ThrowingTransport(this.failure);

  final DAKitException failure;

  @override
  Future<Map<String, Object?>> getJson(
    String path, {
    Map<String, Object?> query = const <String, Object?>{},
    CancelToken? cancelToken,
  }) => Future<Map<String, Object?>>.error(failure);
}
