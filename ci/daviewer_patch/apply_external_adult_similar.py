from pathlib import Path

# Wire the external adult similar-art rail into the artwork detail page.
p = Path("DAViewer/lib/features/artwork/artwork_detail_screen.dart")
s = p.read_text()
old = """import 'more_like_this.dart';
import 'similar_artists.dart';
"""
new = """import 'more_like_this.dart';
import 'external_adult_similar.dart';
import 'similar_artists.dart';
"""
if old not in s:
    raise SystemExit("detail import anchor changed")
s = s.replace(old, new, 1)

old = """          MoreFromArtistSection(artworkId: widget.artworkId),
          MoreLikeThisSection(artworkId: widget.artworkId),
          SimilarArtistsSection(artworkId: widget.artworkId),
"""
new = """          MoreFromArtistSection(artworkId: widget.artworkId),
          MoreLikeThisSection(artworkId: widget.artworkId),
          ExternalAdultSimilarSection(artworkId: widget.artworkId),
          SimilarArtistsSection(artworkId: widget.artworkId),
"""
if old not in s:
    raise SystemExit("detail section anchor changed")
p.write_text(s)

# Add localized labels without changing the app's existing language model.
p = Path("DAViewer/lib/core/l10n/app_strings.dart")
s = p.read_text()
old = """  String get moreLikeThis =>
      _lang == AppLanguage.zh ? '更多类似作品' : 'More like this';
  String get featuredInCollections =>
"""
new = """  String get moreLikeThis =>
      _lang == AppLanguage.zh ? '更多类似作品' : 'More like this';
  String get externalAdultSimilar =>
      _lang == AppLanguage.zh ? '站外成人相似作品' : 'Similar adult art · external';
  String get externalAdultSimilarHint => _lang == AppLanguage.zh
      ? '仅显示 Explicit 结果，并过滤未成年或年龄不明确标签。'
      : 'Explicit-only results with strict underage/age-ambiguous tag filtering.';
  String get featuredInCollections =>
"""
if old not in s:
    raise SystemExit("l10n anchor changed")
p.write_text(s)
