from pathlib import Path

p = Path("DAViewer/lib/features/search/search_providers.dart")
s = p.read_text(encoding="utf-8")

old = """      SourceAttempt(
        source: fallback,
        run: () async {
          try {
            final page = await dataAccessFor(runtime).search(query, request);
            return page.items.isEmpty
                ? SourceResult<Page<Artwork>>.empty(source: fallback)
                : SourceResult<Page<Artwork>>.success(page, source: fallback);
          } on Object catch (error) {
            return SourceResult<Page<Artwork>>.failed(error, source: fallback);
          }
        },
      ),
"""
new = """      SourceAttempt(
        source: fallback,
        run: () async {
          try {
            final page = await _officialSearchFallback(runtime, query, request);
            return page.items.isEmpty
                ? SourceResult<Page<Artwork>>.empty(source: fallback)
                : SourceResult<Page<Artwork>>.success(page, source: fallback);
          } on Object catch (error) {
            return SourceResult<Page<Artwork>>.failed(error, source: fallback);
          }
        },
      ),
"""
if s.count(old) != 1:
    raise SystemExit(f"search fallback block mismatch: {s.count(old)}")
s = s.replace(old, new, 1)

anchor = """Future<SourceResult<Page<Artwork>>> _tryWebSearch(
"""
helper = r"""
const String _tagFallbackCursorPrefix = 'tag-fallback:';

Future<Page<Artwork>> _officialSearchFallback(
  AppRuntime runtime,
  String query,
  PageRequest request,
) async {
  final transport = runtime.transport;
  if (transport == null) {
    throw const DAKitException(
      kind: DAKitFailureKind.configuration,
      code: 'app.runtime.transport',
      message: 'The official API transport is not available.',
    );
  }

  final resumed = _decodeTagFallbackCursor(request.cursor);
  final discovery = OfficialDiscoveryRepository(transport);
  if (resumed != null) {
    final page = await discovery.tag(
      resumed.tag,
      PageRequest(cursor: resumed.cursor, limit: request.limit),
    );
    return _wrapTagFallbackPage(resumed.tag, page);
  }

  final offset = int.tryParse(request.cursor ?? '') ?? 0;
  final json = await transport.getJson(
    'browse/home',
    query: <String, Object?>{
      'offset': offset,
      'limit': request.limit,
      'q': query.trim(),
      'mature_content': true,
    },
  );
  final rawResults = json['results'];
  const mapper = DeviationMapper();
  final items = <Artwork>[];
  if (rawResults is List) {
    for (final raw in rawResults) {
      if (raw is! Map) continue;
      try {
        items.add(
          mapper.artwork(
            raw.map<String, Object?>(
              (key, value) => MapEntry(key.toString(), value),
            ),
          ),
        );
      } on Object {
        // Skip one malformed item without failing the search page.
      }
    }
  }
  final hasMore = json['has_more'] == true;
  final nextOffset = json['next_offset'];
  if (items.isNotEmpty || hasMore || request.cursor != null) {
    return Page<Artwork>(
      items: items,
      hasMore: hasMore,
      nextCursor: hasMore && nextOffset != null ? '$nextOffset' : null,
    );
  }

  List<String> suggestions;
  try {
    suggestions = await discovery.suggestTags(query);
    if (suggestions.isEmpty) {
      final compact = _normalizeTag(query);
      if (compact.isNotEmpty && compact != query.trim().toLowerCase()) {
        suggestions = await discovery.suggestTags(compact);
      }
    }
  } on Object {
    return const Page<Artwork>(items: <Artwork>[], hasMore: false);
  }
  if (suggestions.isEmpty) {
    return const Page<Artwork>(items: <Artwork>[], hasMore: false);
  }

  final normalizedQuery = _normalizeTag(query);
  final selectedTag = suggestions.cast<String?>().firstWhere(
    (tag) => tag != null && _normalizeTag(tag) == normalizedQuery,
    orElse: () => suggestions.first,
  )!;

  final tagPage = await discovery.tag(
    selectedTag,
    PageRequest(limit: request.limit),
  );
  return _wrapTagFallbackPage(selectedTag, tagPage);
}

String _normalizeTag(String value) => value
    .trim()
    .toLowerCase()
    .replaceAll(RegExp(r'[\s_#-]+'), '');

Page<Artwork> _wrapTagFallbackPage(String tag, Page<Artwork> page) {
  final next = page.nextCursor;
  return Page<Artwork>(
    items: page.items,
    hasMore: page.hasMore,
    nextCursor: page.hasMore && next != null
        ? '$_tagFallbackCursorPrefix\${Uri.encodeComponent(tag)}:\${Uri.encodeComponent(next)}'
        : null,
  );
}

({String tag, String? cursor})? _decodeTagFallbackCursor(String? raw) {
  if (raw == null || !raw.startsWith(_tagFallbackCursorPrefix)) return null;
  final payload = raw.substring(_tagFallbackCursorPrefix.length);
  final separator = payload.indexOf(':');
  if (separator <= 0) return null;
  final tag = Uri.decodeComponent(payload.substring(0, separator));
  final encodedCursor = payload.substring(separator + 1);
  if (tag.trim().isEmpty) return null;
  return (
    tag: tag,
    cursor: encodedCursor.isEmpty ? null : Uri.decodeComponent(encodedCursor),
  );
}

"""
if anchor not in s:
    raise SystemExit("search helper anchor changed")
s = s.replace(anchor, helper + anchor, 1)

p.write_text(s, encoding="utf-8")
print("Search resilience patch applied")
