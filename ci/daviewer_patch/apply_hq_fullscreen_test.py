from pathlib import Path

p = Path("DAViewer/test/media_viewer_test.dart")
s = p.read_text()

anchor = """  testWidgets('multi-image pages consume swipes before the next artwork', (
    tester,
  ) async {
"""

test = """  testWidgets('fullscreen uses verified original while inline keeps preview', (
    tester,
  ) async {
    final preview = asset(
      'preview',
      MediaKind.image,
      role: MediaRole.preview,
      width: 800,
      uri: Uri.parse('https://example.test/preview.jpg'),
    );
    final original = asset(
      'original',
      MediaKind.image,
      role: MediaRole.original,
      width: 3200,
      uri: Uri.parse('https://example.test/original.jpg'),
    );

    await tester.pumpWidget(
      ProviderScope(
        child: MaterialApp(
          home: Scaffold(
            body: SizedBox(
              width: 400,
              child: MediaViewer(
                media: <MediaAsset>[preview],
                originalMedia: original,
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    final inline = tester.widget<CachedNetworkImage>(
      find.byType(CachedNetworkImage).first,
    );
    expect(inline.imageUrl, 'https://example.test/preview.jpg');

    final tapTarget = find
        .ancestor(
          of: find.byType(CachedNetworkImage).first,
          matching: find.byType(GestureDetector),
        )
        .first;
    await tester.tap(tapTarget);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 350));

    final viewer = tester.widget<FullScreenImageViewer>(
      find.byType(FullScreenImageViewer),
    );
    final provider = viewer.imageProvider as CachedNetworkImageProvider;
    expect(provider.url, 'https://example.test/original.jpg');
  });

"""

if anchor not in s:
    raise SystemExit("media viewer test anchor changed")
s = s.replace(anchor, test + anchor, 1)
p.write_text(s)
