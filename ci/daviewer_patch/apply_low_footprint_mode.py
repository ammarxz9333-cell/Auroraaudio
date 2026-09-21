from pathlib import Path

# 1) Conservative official-API budget and longer in-memory cache.
p = Path("DAKit/packages/dakit_api/lib/src/http/api_config.dart")
s = p.read_text()
s = s.replace(
"""    this.maxRetries = 4,
    this.initialDelay = const Duration(seconds: 2),
    this.maximumDelay = const Duration(seconds: 30),
    this.minimumSpacing = const Duration(milliseconds: 1200),
    this.freshCacheTtl = const Duration(seconds: 45),
    this.staleCacheTtl = const Duration(minutes: 15),
""",
"""    this.maxRetries = 1,
    this.initialDelay = const Duration(seconds: 5),
    this.maximumDelay = const Duration(seconds: 30),
    this.minimumSpacing = const Duration(milliseconds: 2500),
    this.freshCacheTtl = const Duration(minutes: 10),
    this.staleCacheTtl = const Duration(hours: 24),
""",
1)
p.write_text(s)

# 2) During a provider-enforced cooldown, serve stale cache immediately rather
# than queueing behind the cooldown and issuing another request.
p = Path("DAKit/packages/dakit_api/lib/src/http/official_api_client.dart")
s = p.read_text()

old = """    var cached = readKey == null ? null : _readCache[readKey];
    if (cached != null && _cacheFresh(cached)) {
      return cached.value;
    }
    var refreshed = false;
"""
new = """    var cached = readKey == null ? null : _readCache[readKey];
    if (cached != null && _cacheFresh(cached)) {
      return cached.value;
    }
    if (cached != null &&
        _cacheStaleUsable(cached) &&
        _rateGate.cooldownActive) {
      return cached.value;
    }
    var refreshed = false;
"""
if old not in s:
    raise SystemExit("cooldown cache fast-path anchor changed")
s = s.replace(old, new, 1)

old = """  void blockFor(Duration duration) {
    if (duration.inMicroseconds <= 0) return;
    final candidate = _now().toUtc().add(duration);
    if (_blockedUntil == null || candidate.isAfter(_blockedUntil!)) {
      _blockedUntil = candidate;
    }
  }

  static DateTime? _later(DateTime? first, DateTime? second) {
"""
new = """  bool get cooldownActive {
    final blockedUntil = _blockedUntil;
    return blockedUntil != null && blockedUntil.isAfter(_now().toUtc());
  }

  void blockFor(Duration duration) {
    if (duration.inMicroseconds <= 0) return;
    final candidate = _now().toUtc().add(duration);
    if (_blockedUntil == null || candidate.isAfter(_blockedUntil!)) {
      _blockedUntil = candidate;
    }
  }

  static DateTime? _later(DateTime? first, DateTime? second) {
"""
if old not in s:
    raise SystemExit("rate gate cooldown getter anchor changed")
s = s.replace(old, new, 1)
p.write_text(s)

# 3) Persistent artwork cache: use local data aggressively across app restarts.
p = Path("DAViewer/lib/core/cache/artwork_page_cache.dart")
s = p.read_text()
s = s.replace(
"""  static const Duration defaultFreshFor = Duration(minutes: 2);
  static const Duration defaultStaleFor = Duration(days: 7);
""",
"""  static const Duration defaultFreshFor = Duration(minutes: 30);
  static const Duration defaultStaleFor = Duration(days: 30);
""",
1)
p.write_text(s)

# 4) Existing explicit short per-feed freshness windows become low-footprint
# windows too.
for filename in [
    "DAViewer/lib/features/home/home_providers.dart",
    "DAViewer/lib/features/tag/tag_screen.dart",
]:
    p = Path(filename)
    s = p.read_text()
    s = s.replace("freshFor: const Duration(minutes: 10)", "freshFor: const Duration(minutes: 30)")
    s = s.replace("freshFor: const Duration(minutes: 5)", "freshFor: const Duration(minutes: 30)")
    p.write_text(s)

print("Low-footprint policy installed: 2.5s budget, 10m memory cache, 30m persistent freshness, cooldown cache short-circuit.")
