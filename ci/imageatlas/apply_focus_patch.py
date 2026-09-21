from pathlib import Path
import sys

p = Path(sys.argv[1])
s = p.read_text()

def rep(old, new, label):
    global s
    if old not in s:
        raise SystemExit(f"anchor not found: {label}")
    s = s.replace(old, new, 1)

rep(
"""bool isOpenverseSensitive(Map<String, dynamic> json) {
  if (json['mature'] == true) return true;
  final sensitivity = json['sensitivity'];
  return sensitivity is List && sensitivity.isNotEmpty;
}
""",
"""bool isOpenverseSensitive(Map<String, dynamic> json) {
  if (json['mature'] == true) return true;
  final sensitivity = json['sensitivity'];
  return sensitivity is List && sensitivity.isNotEmpty;
}

const List<String> suggestiveFocusTerms = <String>[
  'glamour',
  'sensual',
  'boudoir',
   'lingerie',
  'pinup',
  'pin-up',
  'model',
  'fashion',
  'swimwear',
  'bikini',
  'portrait',
  'mature',
  'suggestive',
];

List<String> focusedQueries(String raw) {
  final query = raw.trim();
  if (query.isEmpty) return const <String>[];
  return <String>[
    $'query glamour model',
    $'query sensual fashion',
    $'query boudoir portrait',
  ];
}

int focusedContentScore(ImageItem item, String query) {
  final text = '${item.title} ${item.creator}'.toLowerCase();
  var score = item.isSensitive ? 12 : 0;
  for (final term in suggestiveFocusTerms) {
    if (text.contains(term)) score += 5;
  }
  for (final token in query
      .toLowerCase()
      .split(RegExp(r'\\s+'))
      .where((token) => token.isNotEmpty)) {
    if (text.contains(token)) score += 3;
    if (item.title.toLowerCase().contains(token)) score += 2;
  }
  return score;
}
""",
"focus helpers",
)

rep(
"""    final futures = active.map((source) async {
      final page = _pages[source.name] ?? 1;
      try {
        final items = await source.search(
          query: _activeQuery,
          page: page,
          includeSensitive: includeSensitive,
        );
        return (
          source: source,
          page: page,
          items: items,
          error: null as Object?,
        );
      } on Object catch (error) {
        return (
          source: source,
          page: page,
          items: const <ImageItem>[],
          error: error as Object?,
        );
      }
    });

    final results = await Future.wait(futures);
""",
"""    final futures = active.map((source) async {
      final page = _pages[source.name] ?? 1;
      try {
        final collected = <ImageItem>[];
        for (final focusedQuery in focusedQueries(_activeQuery)) {
          final items = await source.search(
            query: focusedQuery,
            page: page,
            includeSensitive: includeSensitive,
          );
          collected.addAll(items);
        }
        return (
          source: source,
          page: page,
          items: dedupeImages(collected),
          error: null as Object?,
        );
      } on Object catch (error) {
        return (
          source: source,
          page: page,
          items: const <ImageItem>[],
          error: error as Object?,
        );
      }
    });

    final results = await Future.wait(futures);
""",
"multi query",
)

rep(
"""    deduped.sort((a, b) {
      return _score(b, _activeQuery).compareTo(_score(a, _activeQuery));
    });
""",
"""    deduped.sort((a, b) {
      return focusedContentScore(
        b,
        _activeQuery,
      ).compareTo(focusedContentScore(a, _activeQuery));
    });
""",
"ranking",
)

score_start = s.index("  int _score(")
score_end = s.index("  @override\n  Widget build", score_start)
s = s[:score_start] + s[score_end:]
rep(
"              hintText: 'Search images across sources',\n",
"              hintText: 'Search mature / suggestive imagery',\n",
"hint",
)

rep(
"""                    title: 'Search several image sources at once',
                    subtitle:
                        'Openverse, Wikimedia Commons and Flickr public photos are enabled.',
""",
"""                    title: 'Focused mature / suggestive discovery',
                    subtitle:
                        'Searches are automatically expanded toward glamour, sensual, boudoir, model and fashion imagery across the enabled sources.',
""",
"empty state",
)

rep(
"""          Text(
            'Sensitive content',
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 8),
          const Text(
            'Uses each source’s supported sensitivity controls. The app does '
            'not bypass logins, private content, age gates or provider rules.',
          ),
          const SizedBox(height: 16),
          SegmentedButton<MatureMode>(
            segments: MatureMode.values
                .map(
                  (mode) => ButtonSegment<MatureMode>(
                    value: mode,
                    label: Text(mode.label),
                  ),
                )
                .toList(growable: false),
            selected: <MatureMode>{matureMode},
            onSelectionChanged: (selection) {
              if (selection.isNotEmpty) {
                unawaited(setMatureMode(selection.first));
              }
            },
          ),
""",
"""          Text(
            'Mature / suggestive focus',
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 8),
          const Text(
            'ImageAtlas is tuned for glamour, sensual, boudoir, model and other '
            'mature/suggestive imagery rather than general image search.',
          ),
          const SizedBox(height: 16),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Include source-labelled sensitive results'),
            subtitle: const Text(
              'Requests sensitive results where the source exposes an official setting.',
            ),
            value: matureMode != MatureMode.hide,
            onChanged: (enabled) {
              unawaited(
                setMatureMode(enabled ? MatureMode.blur : MatureMode.hide),
              );
            },
          ),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Show sensitive thumbnails without blur'),
            subtitle: const Text(
              'Requires adult confirmation when switched on.',
            ),
            value: matureMode == MatureMode.show,
            onChanged: matureMode == MatureMode.hide
                ? null
                : (enabled) {
                    unawaited(
                      setMatureMode(
                        enabled ? MatureMode.show : MatureMode.blur,
                      ),
                    );
                },
          ),
""",
"settings",
)

rep(
"""            title: Text('Broad search'),
            subtitle: Text(
              'Openverse itself indexes multiple upstream media providers, '
              'while the other adapters add direct source coverage.',
            ),
""",
"""            title: Text('Focused search'),
            subtitle: Text(
              'Each query is expanded into multiple glamour/sensual/model-focused '
              'variants before results are merged, deduplicated and ranked.',
            ),
""",
"focused description",
)

p.write_text(s)
