part of '../main.dart';

class LocalStore {
  static Future<List<Artwork>> favorites() async {
    final p = await SharedPreferences.getInstance();
    final raw = p.getString('favorites');
    if (raw == null) return [];
    try {
      return (jsonDecode(raw) as List)
          .map((e) => Artwork.fromJson(Map<String, dynamic>.from(e as Map)))
          .toList();
    } catch (_) {
      return [];
    }
  }

  static Future<bool> isFavorite(Artwork a) async {
    final all = await favorites();
    return all.any((e) => e.source == a.source && e.id == a.id);
  }

  static Future<bool> toggleFavorite(Artwork a) async {
    final p = await SharedPreferences.getInstance();
    final all = await favorites();
    final i = all.indexWhere((e) => e.source == a.source && e.id == a.id);
    final now;
    if (i >= 0) {
      all.removeAt(i);
      now = false;
    } else {
      all.insert(0, a);
      now = true;
    }
    await p.setString('favorites', jsonEncode(all.map((e) => e.toJson()).toList()));
    return now;
  }

  static Future<List<Map<String, dynamic>>> downloads() async {
    final p = await SharedPreferences.getInstance();
    final raw = p.getString('downloads');
    if (raw == null) return [];
    try {
      return (jsonDecode(raw) as List)
          .map((e) => Map<String, dynamic>.from(e as Map))
          .toList();
    } catch (_) {
      return [];
    }
  }

  static Future<void> addDownload(Map<String, dynamic> d) async {
    final p = await SharedPreferences.getInstance();
    final all = await downloads();
    all.insert(0, d);
    await p.setString('downloads', jsonEncode(all.take(200).toList()));
  }
}

class DownloadService {
  static String safeName(String s) =>
      s.replaceAll(RegExp(r'[^a-zA-Z0-9._-]+'), '_').replaceAll(RegExp(r'_+'), '_');

  static Future<List<String>> download(Artwork a) async {
    final base = await getExternalStorageDirectory();
    if (base == null) throw Exception('Storage unavailable');
    final dir = Directory('${base.path}/PixVault');
    await dir.create(recursive: true);

    final urls = a.pageUrls.isNotEmpty
        ? a.pageUrls
        : (a.mediaUrl.isNotEmpty ? [a.mediaUrl] : [a.previewUrl]);
    final saved = <String>[];
    final pixiv = PixivRepo();

    for (var i = 0; i < urls.length; i++) {
      final u = urls[i];
      if (u.isEmpty) continue;
      final r = await http.get(
        Uri.parse(u),
        headers: a.source == SourceType.pixiv
            ? await pixiv.headers(referer: a.sourceUrl)
            : mediaHeaders(a),
      );
      if (r.statusCode != 200) throw Exception('Download HTTP ${r.statusCode}');
      var ext = Uri.parse(u).path.split('.').last.toLowerCase();
      if (ext.length > 5 || ext.contains('/')) ext = a.isVideo ? 'mp4' : 'jpg';
      final path =
          '${dir.path}/${a.source.short}_${safeName(a.id)}_${i + 1}.$ext';
      await File(path).writeAsBytes(r.bodyBytes);
      saved.add(path);
      await LocalStore.addDownload({
        'title': a.title,
        'source': a.source.label,
        'path': path,
        'date': DateTime.now().toIso8601String(),
      });
    }
    return saved;
  }
}
