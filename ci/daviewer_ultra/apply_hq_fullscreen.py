from pathlib import Path

root = Path("DAViewer")

def replace_once(rel, old, new):
    p = root / rel
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{rel}: expected 1 match, got {count}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")

replace_once(
    "lib/features/artwork/media_viewer.dart",
    """    this.additionalMedia = const <MediaAsset>[],
    this.heroTag,
""",
    """    this.additionalMedia = const <MediaAsset>[],
    this.originalMedia,
    this.additionalOriginalMedia = const <MediaAsset>[],
    this.heroTag,
""",
)
replace_once(
    "lib/features/artwork/media_viewer.dart",
    """  final List<MediaAsset> additionalMedia;

  /// Hero tag""",
    """  final List<MediaAsset> additionalMedia;
  final MediaAsset? originalMedia;
  final List<MediaAsset> additionalOriginalMedia;

  /// Hero tag""",
)
replace_once(
    "lib/features/artwork/media_viewer.dart",
    """  List<MediaAsset> get _pages => <MediaAsset>[
    ?selectDisplayAsset(widget.media),
    ...widget.additionalMedia,
  ];
""",
    """  List<MediaAsset> get _pages => <MediaAsset>[
    ?selectDisplayAsset(widget.media),
    ...widget.additionalMedia,
  ];

  List<MediaAsset> _fullScreenPagesFor(List<MediaAsset> pages) {
    final result = <MediaAsset>[];
    for (var index = 0; index < pages.length; index += 1) {
      final display = pages[index];
      if (!_canOpenFullScreen(display)) continue;

      MediaAsset? replacement;
      if (index == 0) {
        replacement = widget.originalMedia;
      } else {
        final originalIndex = index - 1;
        if (originalIndex < widget.additionalOriginalMedia.length) {
          replacement = widget.additionalOriginalMedia[originalIndex];
        }
      }

      if (replacement != null && _canOpenFullScreen(replacement)) {
        result.add(replacement);
      } else {
        result.add(display);
      }
    }
    return result;
  }

  int _fullScreenIndexFor(List<MediaAsset> pages, int pageIndex) {
    var result = 0;
    for (var index = 0; index < pageIndex; index += 1) {
      if (_canOpenFullScreen(pages[index])) result += 1;
    }
    return result;
  }
""",
)
replace_once(
    "lib/features/artwork/media_viewer.dart",
    """    final pages = _pages;
    final fullScreenPages = pages.where(_canOpenFullScreen).toList();
""",
    """    final pages = _pages;
    final fullScreenPages = _fullScreenPagesFor(pages);
""",
)
replace_once(
    "lib/features/artwork/media_viewer.dart",
    """      return _pageWidget(
        pages.first,
        heroTag: widget.heroTag,
        fullScreenPages: fullScreenPages,
      );
""",
    """      return _pageWidget(
        pages.first,
        heroTag: widget.heroTag,
        fullScreenPages: fullScreenPages,
        fullScreenIndex: 0,
      );
""",
)
replace_once(
    "lib/features/artwork/media_viewer.dart",
    """                  itemBuilder: (context, index) => _pageWidget(
                    pages[index],
                    heroTag: index == 0 ? widget.heroTag : null,
                    fullScreenPages: fullScreenPages,
                  ),
""",
    """                  itemBuilder: (context, index) => _pageWidget(
                    pages[index],
                    heroTag: index == 0 ? widget.heroTag : null,
                    fullScreenPages: fullScreenPages,
                    fullScreenIndex: _fullScreenIndexFor(pages, index),
                  ),
""",
)
replace_once(
    "lib/features/artwork/media_viewer.dart",
    """  Widget _pageWidget(
    MediaAsset asset, {
    String? heroTag,
    required List<MediaAsset> fullScreenPages,
  }) {
""",
    """  Widget _pageWidget(
    MediaAsset asset, {
    String? heroTag,
    required List<MediaAsset> fullScreenPages,
    required int fullScreenIndex,
  }) {
""",
)

p = root / "lib/features/artwork/media_viewer.dart"
text = p.read_text(encoding="utf-8")
count = text.count("initialPage: fullScreenPages.indexOf(asset),")
if count != 2:
    raise SystemExit(f"media viewer fullscreen index anchors: {count}")
p.write_text(
    text.replace(
        "initialPage: fullScreenPages.indexOf(asset),",
        "initialPage: fullScreenIndex,",
    ),
    encoding="utf-8",
)

replace_once(
    "lib/features/artwork/artwork_detail_screen.dart",
    """              additionalMedia: additionalMedia,
              heroTag:""",
    """              additionalMedia: additionalMedia,
              originalMedia: originalResolution.valueOrNull?.asset,
              additionalOriginalMedia: additionalOriginals,
              heroTag:""",
)

test = root / "test/media_viewer_test.dart"
s = test.read_text(encoding="utf-8")
anchor = """  testWidgets('multi-image pages consume swipes before the next artwork', (
    tester,
  ) async {
"""
addition = """  testWidgets('fullscreen uses verified original while inline keeps preview', (
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
test.write_text(s.replace(anchor, addition + anchor, 1), encoding="utf-8")

print("HQ fullscreen patch applied")
