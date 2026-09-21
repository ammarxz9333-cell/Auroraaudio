from pathlib import Path

p = Path("DAViewer/lib/features/artwork/artwork_detail_screen.dart")
s = p.read_text()

old = """      final manager = ref.read(runtimeProvider).transfers;
      final transferId = id ?? 'artwork-${widget.artworkId}-original';
      await manager.initialize();
      final existing = (await manager.records())
          .where((record) => record.id == transferId)
          .firstOrNull;
      if (existing != null) {
        if (const <TransferState>{
          TransferState.failed,
          TransferState.notFound,
          TransferState.cancelled,
        }.contains(existing.state)) {
          await manager.remove(transferId);
        } else {
          if (!mounted) return;
          _showTransferMessage(existing);
          if (track) await _trackTransfer(manager, existing);
          return;
        }
      }
      final snapshot = await manager.enqueue(
        TransferRequest(
          id: transferId,
          asset: original,
          filename: original.filename,
        ),
      );
"""
new = """      final manager = ref.read(runtimeProvider).transfers;
      final transferId = id ?? 'artwork-${widget.artworkId}-original';
      await manager.initialize();

      // Fast path: enqueue() already performs an indexed recordForId lookup in
      // the native transfer database. Avoid loading every historical download
      // before each new transfer; that became progressively slower as the
      // download history grew.
      TransferSnapshot snapshot;
      var reusedExisting = false;
      try {
        snapshot = await manager.enqueue(
          TransferRequest(
            id: transferId,
            asset: original,
            filename: original.filename,
          ),
        );
      } on DAKitException catch (error) {
        if (error.code != 'transfer.id.duplicate') rethrow;

        // The full records read is now a duplicate-only slow path. Re-opening
        // an existing transfer is rare compared with starting a new download.
        final existing = (await manager.records())
            .where((record) => record.id == transferId)
            .firstOrNull;
        if (existing == null) rethrow;

        if (const <TransferState>{
          TransferState.failed,
          TransferState.notFound,
          TransferState.cancelled,
        }.contains(existing.state)) {
          await manager.remove(transferId);
          snapshot = await manager.enqueue(
            TransferRequest(
              id: transferId,
              asset: original,
              filename: original.filename,
            ),
          );
        } else {
          snapshot = existing;
          reusedExisting = true;
        }
      }

      if (reusedExisting) {
        if (!mounted) return;
        _showTransferMessage(snapshot);
        if (track) await _trackTransfer(manager, snapshot);
        return;
      }
"""
if old not in s:
    raise SystemExit("download history fast-path anchor changed")
s = s.replace(old, new, 1)

old = """      if (pages.isNotEmpty) {
        final existingIds = (await manager.records())
            .map((record) => record.id)
            .toSet();
        for (var index = 0; index < pages.length; index++) {
          final page = pages[index];
          if (!page.canTransfer) continue;
          final pageId = imageTransferId(
            widget.artworkId,
            page.id.isEmpty ? 'page:${index + 1}' : page.id,
          );
          if (existingIds.contains(pageId)) continue;
          existingIds.add(pageId);
          await manager.enqueue(
            TransferRequest(id: pageId, asset: page, filename: page.filename),
          );
        }
      }
"""
new = """      if (pages.isNotEmpty) {
        for (var index = 0; index < pages.length; index++) {
          final page = pages[index];
          if (!page.canTransfer) continue;
          final pageId = imageTransferId(
            widget.artworkId,
            page.id.isEmpty ? 'page:${index + 1}' : page.id,
          );
          try {
            await manager.enqueue(
              TransferRequest(id: pageId, asset: page, filename: page.filename),
            );
          } on DAKitException catch (error) {
            // enqueue() already checks this ID directly in the native database.
            // Existing pages are skipped without scanning the entire history.
            if (error.code != 'transfer.id.duplicate') rethrow;
          }
        }
      }
"""
if old not in s:
    raise SystemExit("multi-page history scan anchor changed")
s = s.replace(old, new, 1)

p.write_text(s)

final_text = p.read_text()
download_start = final_text.index("  Future<void> _download(")
download_end = final_text.index("  Future<void> _confirmAndDownloadImage", download_start)
download_body = final_text[download_start:download_end]
if download_body.count("manager.records()") != 1:
    raise SystemExit("download fast path must keep only one duplicate-only records() call")
if "error.code != 'transfer.id.duplicate'" not in download_body:
    raise SystemExit("duplicate fallback missing")
print("Download fast path installed: full history read only on duplicate IDs.")
