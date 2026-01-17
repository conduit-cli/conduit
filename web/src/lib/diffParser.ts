export interface DiffLine {
  type: 'add' | 'remove' | 'context' | 'header' | 'hunk';
  content: string;
  oldLineNumber?: number;
  newLineNumber?: number;
}

export interface DiffFile {
  oldPath: string;
  newPath: string;
  lines: DiffLine[];
  additions: number;
  deletions: number;
}

export interface ParsedDiff {
  files: DiffFile[];
}

export function parseDiff(diffText: string): ParsedDiff {
  const lines = diffText.split('\n');
  const files: DiffFile[] = [];
  let currentFile: DiffFile | null = null;
  let oldLineNum = 0;
  let newLineNum = 0;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // File header: diff --git a/path b/path
    if (line.startsWith('diff --git')) {
      if (currentFile) {
        files.push(currentFile);
      }
      const match = line.match(/diff --git a\/(.+) b\/(.+)/);
      currentFile = {
        oldPath: match?.[1] ?? '',
        newPath: match?.[2] ?? '',
        lines: [],
        additions: 0,
        deletions: 0,
      };
      currentFile.lines.push({ type: 'header', content: line });
      continue;
    }

    if (!currentFile) continue;

    // --- a/path or +++ b/path
    if (line.startsWith('---') || line.startsWith('+++')) {
      currentFile.lines.push({ type: 'header', content: line });
      continue;
    }

    // Index or other git metadata
    if (line.startsWith('index ') || line.startsWith('new file') || line.startsWith('deleted file')) {
      currentFile.lines.push({ type: 'header', content: line });
      continue;
    }

    // Hunk header: @@ -start,count +start,count @@
    if (line.startsWith('@@')) {
      const match = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (match) {
        oldLineNum = parseInt(match[1], 10);
        newLineNum = parseInt(match[2], 10);
      }
      currentFile.lines.push({ type: 'hunk', content: line });
      continue;
    }

    // Addition
    if (line.startsWith('+')) {
      currentFile.lines.push({
        type: 'add',
        content: line.slice(1),
        newLineNumber: newLineNum,
      });
      currentFile.additions++;
      newLineNum++;
      continue;
    }

    // Deletion
    if (line.startsWith('-')) {
      currentFile.lines.push({
        type: 'remove',
        content: line.slice(1),
        oldLineNumber: oldLineNum,
      });
      currentFile.deletions++;
      oldLineNum++;
      continue;
    }

    // Context line (starts with space)
    if (line.startsWith(' ') || line === '') {
      currentFile.lines.push({
        type: 'context',
        content: line.slice(1) || '',
        oldLineNumber: oldLineNum,
        newLineNumber: newLineNum,
      });
      oldLineNum++;
      newLineNum++;
      continue;
    }
  }

  if (currentFile) {
    files.push(currentFile);
  }

  return { files };
}

export function isDiffContent(text: string): boolean {
  // Check for common diff indicators
  const diffPatterns = [
    /^diff --git/m,
    /^@@\s+-\d+/m,
    /^---\s+a\//m,
    /^\+\+\+\s+b\//m,
  ];

  return diffPatterns.some(pattern => pattern.test(text));
}

export function getDiffStats(diff: ParsedDiff): { additions: number; deletions: number } {
  return diff.files.reduce(
    (acc, file) => ({
      additions: acc.additions + file.additions,
      deletions: acc.deletions + file.deletions,
    }),
    { additions: 0, deletions: 0 }
  );
}
