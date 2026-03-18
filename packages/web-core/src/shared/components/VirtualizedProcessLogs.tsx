import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso';
import { WarningCircleIcon } from '@phosphor-icons/react/dist/ssr';
import RawLogText from '@/shared/components/RawLogText';
import type { PatchType } from 'shared/types';

export type LogEntry = Extract<
  PatchType,
  { type: 'STDOUT' } | { type: 'STDERR' }
>;

export interface VirtualizedProcessLogsProps {
  logs: Array<LogEntry>;
  error: string | null;
  searchQuery: string;
  matchIndices: Array<number>;
  currentMatchIndex: number;
}

type LogEntryWithKey = LogEntry & { key: string; originalIndex: number };

interface SearchContext {
  searchQuery: string;
  matchIndices: Array<number>;
  currentMatchIndex: number;
}

const ItemContent = ({
  data,
  context,
}: {
  data: LogEntryWithKey;
  context: SearchContext;
}) => {
  const isMatch = context.matchIndices.includes(data.originalIndex);
  const isCurrentMatch =
    context.matchIndices[context.currentMatchIndex] === data.originalIndex;

  return (
    <RawLogText
      content={data.content}
      channel={data.type === 'STDERR' ? 'stderr' : 'stdout'}
      className="text-sm px-4 py-1"
      linkifyUrls
      searchQuery={isMatch ? context.searchQuery : undefined}
      isCurrentMatch={isCurrentMatch}
    />
  );
};

export function VirtualizedProcessLogs({
  logs,
  error,
  searchQuery,
  matchIndices,
  currentMatchIndex,
}: VirtualizedProcessLogsProps) {
  const { t } = useTranslation('tasks');
  const [logRows, setLogRows] = useState<Array<LogEntryWithKey>>([]);
  const listRef = useRef<VirtuosoHandle | null>(null);
  const hasInitializedRef = useRef(false);
  const prevCurrentMatchRef = useRef<number | undefined>(undefined);
  const isAtBottomRef = useRef(true);
  const previousLengthRef = useRef(0);

  useEffect(() => {
    const nextRows: Array<LogEntryWithKey> = logs.map((entry, index) => ({
      ...entry,
      key: `log-${index}`,
      originalIndex: index,
    }));
    setLogRows(nextRows);
  }, [logs]);

  // Keep list pinned to the latest row only while the user remains at bottom.
  useEffect(() => {
    const hasNewRows = logRows.length > previousLengthRef.current;
    if (logRows.length === 0) {
      previousLengthRef.current = 0;
      return;
    }

    const scrollToLastRow = () => {
      listRef.current?.scrollToIndex({
        index: logRows.length - 1,
        align: 'end',
        behavior: 'auto',
      });
    };

    if (!hasInitializedRef.current) {
      hasInitializedRef.current = true;
      requestAnimationFrame(scrollToLastRow);
    } else if (hasNewRows && isAtBottomRef.current) {
      requestAnimationFrame(scrollToLastRow);
    }

    previousLengthRef.current = logRows.length;
  }, [logRows]);

  // Scroll to current match when it changes.
  useEffect(() => {
    if (
      matchIndices.length > 0 &&
      currentMatchIndex >= 0 &&
      currentMatchIndex !== prevCurrentMatchRef.current
    ) {
      const logIndex = matchIndices[currentMatchIndex];
      listRef.current?.scrollToIndex({
        index: logIndex,
        align: 'center',
        behavior: 'smooth',
      });
      prevCurrentMatchRef.current = currentMatchIndex;
    }
  }, [currentMatchIndex, matchIndices]);

  if (logs.length === 0 && !error) {
    return (
      <div className="h-full flex items-center justify-center">
        <p className="text-center text-muted-foreground text-sm">
          {t('processes.noLogsAvailable')}
        </p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="h-full flex items-center justify-center">
        <p className="text-center text-destructive text-sm">
          <WarningCircleIcon className="size-icon-base inline mr-2" />
          {error}
        </p>
      </div>
    );
  }

  const context: SearchContext = {
    searchQuery,
    matchIndices,
    currentMatchIndex,
  };

  return (
    <div className="h-full overflow-hidden">
      <Virtuoso<LogEntryWithKey>
        ref={listRef}
        className="h-full"
        data={logRows}
        computeItemKey={(_, item) => item.key}
        atBottomStateChange={(isAtBottom) => {
          isAtBottomRef.current = isAtBottom;
        }}
        itemContent={(_, row) => <ItemContent data={row} context={context} />}
      />
    </div>
  );
}
