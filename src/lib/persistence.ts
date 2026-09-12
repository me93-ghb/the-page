export type SaveResult = { saved: boolean; recovered: boolean; base: string | null; cause: string | null; conflicts: string[] };
type Revision = { dirty: boolean; revision: number; recoveredRevision?: number; base: string | null };

export async function persistPages<T extends Revision>(pages: T[], write: (page: T) => Promise<SaveResult>) {
  const results: SaveResult[] = [];
  for (const page of pages) {
    const conflicts = new Set<string>();
    while (page.dirty) {
      const revision = page.revision;
      const result = await write(page);
      for (const path of result.conflicts) conflicts.add(path);
      if (result.saved) page.base = result.base;
      page.recoveredRevision = result.recovered || result.saved ? revision : -1;
      if (result.saved && revision === page.revision) page.dirty = false;
      if (revision === page.revision) { results.push({ ...result, conflicts: [...conflicts] }); break; }
    }
  }
  return { durable: pages.every(page => !page.dirty || page.recoveredRevision === page.revision), results };
}
