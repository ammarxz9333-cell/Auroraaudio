import 'package:daviewer/features/home/daily_archive_provider.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('daily cursor walks across month and leap year', () {
    expect(previousDailyCursor('2026-03-01'), '2026-02-28');
    expect(previousDailyCursor('2024-03-01'), '2024-02-29');
    expect(previousDailyCursor('2026-01-01'), '2025-12-31');
  });

  test('daily archive stops at the beginning of DeviantArt history', () {
    expect(previousDailyCursor('2000-01-01'), isNull);
    expect(previousDailyCursor('2000-01-02'), '2000-01-01');
  });

  test('malformed cursor rejects rather than silently repeating a page', () {
    expect(() => previousDailyCursor('not-a-date'), throwsFormatException);
  });
}
