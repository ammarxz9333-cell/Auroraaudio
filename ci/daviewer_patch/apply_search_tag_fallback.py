from pathlib import Path

p = Path("DAViewer/lib/features/search/search_providers.dart")
s = p.read_text()

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
            final page = await _officialSearchFallback(
              runtime,
              query,
              request,
            );
            return page.items.isEmpty
                ? SourceResult<Page<Artwork>>.empty(source: fallback)
                : SourceResult<Page<Artwork>>.success(page, source: fallback);
          } on Object catch (error) {
            return SourceResult<Page<Artwork>>.failed(error, source: fallback);
          }
        },
      ),
"""
if old not in s:
    raise SystemExit("search fallback block changed upstream")
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
  final discovery = OfficialDiscoveryRepository(runtime.transport!);
  final resumed = _decodeTagFallbackCursor(request.cursor);
  if (resumed != null) {
    final page = await discovery.tag(
      resumed.tag,
      PageRequest(cursor: resumed.cursor, limit: request.limit),
    );
    return _wrapTagFallbackPage(resumed.tag, page);
  }

  final page = await dataAccessFor(runtime).search(query, request);
  if (page.items.isNotEmpty || page.hasMore || request.cursor != null) {
    return page;
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
    return page;
  }
  if (suggestions.isEmpty) return page;

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
        ? '$_tagFallbackCursorPrefix${Uri.encodeComponent(tag)}:${Uri.encodeComponent(next)}'
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
    raise SystemExit("web search anchor changed upstream")
s = s.replace(anchor, helper + anchor, 1)

p.write_text(s)
