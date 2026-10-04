import 'package:flutter_test/flutter_test.dart';
import 'package:pixvault/main.dart';

void main() {
  group('Safety', () {
    test('blocks explicit minor-indicating queries', () {
      expect(Safety.blockedQuery('loli'), isTrue);
      expect(Safety.blockedQuery('underage'), isTrue);
      expect(Safety.blockedQuery('landscape'), isFalse);
    });

    test('blocks artwork carrying a blocked tag', () {
      final item = Artwork(
        source: SourceType.yandere,
        id: '1',
        title: 'sample',
        userName: '',
        previewUrl: '',
        mediaUrl: '',
        isVideo: false,
        tags: const ['minor'],
        pageUrls: const [],
        sourceUrl: '',
      );
      expect(Safety.blockedArtwork(item), isTrue);
    });
  });
}
