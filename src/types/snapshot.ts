/*
 * Purpose: Snapshot browsing types matching `snapshot_cmd` serde payloads.
 * Role: Timeline rows and lazy directory listings for the file tree.
 */

/** Remote snapshot directory metadata. */
export type SnapshotEntry = {
  name: string;
};

/** One child returned by `list_files`. */
export type FileEntry = {
  name: string;
  is_dir: boolean;
  size: number;
  modified_unix: number | null;
};

/** UTF-8 snapshot file preview returned by `read_snapshot_file`. */
export type SnapshotFileContents = {
  text: string;
  truncated: boolean;
};

/** One project's destinations after `restore_all_projects`. */
export type RestoreEveryProjectRow = {
  project: string;
  destinations: string[];
};
