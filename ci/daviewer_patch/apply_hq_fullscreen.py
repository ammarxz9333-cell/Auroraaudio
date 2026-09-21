from pathlib import Path

p = Path("DAViewer/lib/features/artwork/media_viewer.dart")
s = p.read_text()

s = s.replace(
"""    this.additionalMedia = const <MediaAsset>[],
    this.heroTag,
""",
"""    this.additionalMedia = const <MediaAsset>[],
    this.originalMedia,
    this.additionalOriginalMedia = const <MediaAsset>[],
    this.heroTag,
""",
1)

s = s.replace(
"""  final List<MediaAsset> additionalMedia;

  /// Hero tag""",
"""  final List<MediaAsset> additionalMedia;
  final MediaAsset? originalMedia;
  final List<MediaAsset> additionalOriginalMedia;

  /// Hero tag""",
1)

s = s.replace(
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
1)

s = s.replace(
"""    final pages = _pages;
    final fullScreenPages = pages.where(_canOpenFullScreen).toList();
""",
"""    final pages = _pages;
    final fullScreenPages = _fullScreenPagesFor(pages);
""",
1)

s = s.replace(
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
1)

s = s.replace(
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
1)

s = s.replace(
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
1)

s = s.replace(
"""        initialPage: fullScreenPages.indexOf(asset),
""",
"""        initialPage: fullScreenIndex,
""",
2)

p.write_text(s)

p = Path("DAViewer/lib/features/artwork/artwork_detail_screen.dart")
s = p.read_text()

old = """            MediaViewer(
              media: media,
              additionalMedia: additionalMedia,
              heroTag: 'artwork-${artwork.id}',
"""
new = """            MediaViewer(
              media: media,
              additionalMedia: additionalMedia,
              originalMedia: originalResolution.valueOrNull?.asset,
              additionalOriginalMedia: additionalOriginals,
              heroTag: 'artwork-${artwork.id}',
"""
if old not in s:
    raise SystemExit("artwork detail MediaViewer anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)
