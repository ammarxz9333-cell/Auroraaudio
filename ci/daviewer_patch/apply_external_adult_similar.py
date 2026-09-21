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


# Fail fast if the detail-page wiring was not applied.
final = Path("DAViewer/lib/features/artwork/artwork_detail_screen.dart").read_text()
if "ExternalAdultSimilarSection(artworkId: widget.artworkId)" not in final:
    raise SystemExit("external adult section wiring missing")
