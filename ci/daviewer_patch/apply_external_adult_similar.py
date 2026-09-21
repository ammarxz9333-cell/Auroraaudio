from pathlib import Path

p = Path("DAViewer/lib/features/artwork/artwork_detail_screen.dart")
s = p.read_text()

import_line = "import 'external_adult_similar.dart';"
if import_line not in s:
    anchor = "import 'similar_artists.dart';"
    if anchor not in s:
        raise SystemExit("similar_artists import anchor missing")
    s = s.replace(anchor, import_line + "\n" + anchor, 1)

section_line = "          ExternalAdultSimilarSection(artworkId: widget.artworkId),"
if section_line not in s:
    anchor = "          MoreLikeThisSection(artworkId: widget.artworkId),"
    if anchor not in s:
        raise SystemExit("MoreLikeThis section anchor missing")
    s = s.replace(anchor, anchor + "\n" + section_line, 1)

p.write_text(s)

final_text = p.read_text()
print("external import present:", import_line in final_text)
print("external section present:", section_line in final_text)
