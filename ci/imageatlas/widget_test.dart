import 'package:flutter_test/flutter_test.dart';
import 'package:imageatlas/main.dart';

void main() {
  test('Openverse sensitivity parser accepts known shapes', () {
    expect(isOpenverseSensitive(<String, dynamic>{'mature': true}), isTrue);
    expect(
      isOpenverseSensitive(<String, dynamic>{'sensitivity': <String>['mature']}),
      isTrue,
    );
    expect(isOpenverseSensitive(<String, dynamic>{'sensitivity': <dynamic>[]}), isFalse);
  });

  test('dedupe keeps unique full URLs', () {
    const a = ImageItem(
      id: '1', source: 'A', title: 'x', creator: '',
      thumbnailUrl: 'https://a.test/1.jpg',
      fullUrl: 'https://a.test/full.jpg',
      pageUrl: 'https://a.test/item',
    );
    const b = ImageItem(
      id: '2', source: 'B', title: 'y', creator: '',
      thumbnailUrl: 'https://b.test/2.jpg',
      fullUrl: 'https://a.test/full.jpg',
      pageUrl: 'https://b.test/item',
    );
    expect(dedupeImages(<ImageItem>[a, b]), hasLength(1));
  });
}
