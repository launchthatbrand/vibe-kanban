import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso';
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react';
import { SpinnerIcon } from '@phosphor-icons/react';

import { cn } from '@/shared/lib/utils';
import DisplayConversationEntry from './DisplayConversationEntry';
import { ApprovalFormProvider } from '@/shared/hooks/ApprovalForm';
import { useEntries } from '../model/contexts/EntriesContext';
import {
  useResetProcess,
  type UseResetProcessResult,
} from '../model/hooks/useResetProcess';
import type {
  AddEntryType,
  PatchTypeWithKey,
  DisplayEntry,
} from '@/shared/hooks/useConversationHistory/types';
import {
  isAggregatedGroup,
  isAggregatedDiffGroup,
  isAggregatedThinkingGroup,
} from '@/shared/hooks/useConversationHistory/types';
import { useConversationHistory } from '../model/hooks/useConversationHistory';
import { aggregateConsecutiveEntries } from '@/shared/lib/aggregateEntries';
import type { WorkspaceWithSession } from '@/shared/types/attempt';
import type { RepoWithTargetBranch } from 'shared/types';
import { useWorkspaceContext } from '@/shared/hooks/useWorkspaceContext';
import { ChatScriptPlaceholder } from '@vibe/ui/components/ChatScriptPlaceholder';
import { ScriptFixerDialog } from '@/shared/dialogs/scripts/ScriptFixerDialog';

interface ConversationListProps {
  attempt: WorkspaceWithSession;
  onAtBottomChange?: (atBottom: boolean) => void;
}

export interface ConversationListHandle {
  scrollToPreviousUserMessage: () => void;
  scrollToBottom: () => void;
}

interface MessageListContext {
  attempt: WorkspaceWithSession;
  onConfigureSetup: (() => void) | undefined;
  onConfigureCleanup: (() => void) | undefined;
  showSetupPlaceholder: boolean;
  showCleanupPlaceholder: boolean;
  resetAction: UseResetProcessResult;
}

const ItemContent = ({
  data,
  context,
}: {
  data: DisplayEntry;
  context: MessageListContext;
}) => {
  const attempt = context.attempt;
  const resetAction = context.resetAction;

  // Handle aggregated tool groups (file_read, search, web_fetch)
  if (isAggregatedGroup(data)) {
    return (
      <DisplayConversationEntry
        expansionKey={data.patchKey}
        aggregatedGroup={data}
        aggregatedDiffGroup={null}
        aggregatedThinkingGroup={null}
        entry={null}
        executionProcessId={data.executionProcessId}
        workspaceWithSession={attempt}
        resetAction={resetAction}
      />
    );
  }

  // Handle aggregated diff groups (file_edit by same path)
  if (isAggregatedDiffGroup(data)) {
    return (
      <DisplayConversationEntry
        expansionKey={data.patchKey}
        aggregatedGroup={null}
        aggregatedDiffGroup={data}
        aggregatedThinkingGroup={null}
        entry={null}
        executionProcessId={data.executionProcessId}
        workspaceWithSession={attempt}
        resetAction={resetAction}
      />
    );
  }

  // Handle aggregated thinking groups (thinking entries in previous turns)
  if (isAggregatedThinkingGroup(data)) {
    return (
      <DisplayConversationEntry
        expansionKey={data.patchKey}
        aggregatedGroup={null}
        aggregatedDiffGroup={null}
        aggregatedThinkingGroup={data}
        entry={null}
        executionProcessId={data.executionProcessId}
        workspaceWithSession={attempt}
        resetAction={resetAction}
      />
    );
  }

  if (data.type === 'STDOUT') {
    return <p>{data.content}</p>;
  }
  if (data.type === 'STDERR') {
    return <p>{data.content}</p>;
  }
  if (data.type === 'NORMALIZED_ENTRY' && attempt) {
    return (
      <DisplayConversationEntry
        expansionKey={data.patchKey}
        entry={data.content}
        aggregatedGroup={null}
        aggregatedDiffGroup={null}
        aggregatedThinkingGroup={null}
        executionProcessId={data.executionProcessId}
        workspaceWithSession={attempt}
        resetAction={resetAction}
      />
    );
  }

  return null;
};

type PendingScrollAction = 'initial' | 'plan' | 'running' | 'default' | null;

export const ConversationList = forwardRef<
  ConversationListHandle,
  ConversationListProps
>(function ConversationList({ attempt, onAtBottomChange }, ref) {
  const resetAction = useResetProcess();
  const [displayEntries, setDisplayEntries] = useState<Array<DisplayEntry>>([]);
  const [loading, setLoading] = useState(true);
  const [pendingScrollAction, setPendingScrollAction] =
    useState<PendingScrollAction>(null);
  const { setEntries, reset } = useEntries();
  const pendingUpdateRef = useRef<{
    entries: Array<PatchTypeWithKey>;
    addType: AddEntryType;
    loading: boolean;
  } | null>(null);
  const debounceTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const messageListRef = useRef<VirtuosoHandle | null>(null);
  const isAtBottomRef = useRef(true);
  const visibleStartIndexRef = useRef(0);
  const previousLengthRef = useRef(0);
  const loadingRef = useRef(true);

  // Get repos from workspace context to check if scripts are configured
  let repos: RepoWithTargetBranch[] = [];
  try {
    const workspaceContext = useWorkspaceContext();
    repos = workspaceContext.repos;
  } catch {
    // Context not available
  }

  // Use ref to access current repos without causing callback recreation
  const reposRef = useRef(repos);
  reposRef.current = repos;

  // Check if any repo has setup or cleanup scripts configured
  const hasSetupScript = repos.some((repo) => repo.setup_script);
  const hasCleanupScript = repos.some((repo) => repo.cleanup_script);

  // Handlers to open script fixer dialog for setup/cleanup scripts
  const handleConfigureSetup = useCallback(() => {
    const currentRepos = reposRef.current;
    if (currentRepos.length === 0) return;

    ScriptFixerDialog.show({
      scriptType: 'setup',
      repos: currentRepos,
      workspaceId: attempt.id,
      sessionId: attempt.session?.id,
    });
  }, [attempt.id, attempt.session?.id]);

  const handleConfigureCleanup = useCallback(() => {
    const currentRepos = reposRef.current;
    if (currentRepos.length === 0) return;

    ScriptFixerDialog.show({
      scriptType: 'cleanup',
      repos: currentRepos,
      workspaceId: attempt.id,
      sessionId: attempt.session?.id,
    });
  }, [attempt.id, attempt.session?.id]);

  // Determine if configure buttons should be shown
  const canConfigure = repos.length > 0;

  useEffect(() => {
    setLoading(true);
    loadingRef.current = true;
    setDisplayEntries([]);
    setPendingScrollAction(null);
    reset();
  }, [attempt.id, reset]);

  useEffect(() => {
    return () => {
      if (debounceTimeoutRef.current) {
        clearTimeout(debounceTimeoutRef.current);
      }
    };
  }, []);

  const onEntriesUpdated = (
    newEntries: Array<PatchTypeWithKey>,
    addType: AddEntryType,
    newLoading: boolean
  ) => {
    pendingUpdateRef.current = {
      entries: newEntries,
      addType,
      loading: newLoading,
    };

    if (debounceTimeoutRef.current) {
      clearTimeout(debounceTimeoutRef.current);
    }

    debounceTimeoutRef.current = setTimeout(() => {
      const pending = pendingUpdateRef.current;
      if (!pending) return;

      let nextScrollAction: PendingScrollAction;

      if (loadingRef.current) {
        nextScrollAction = 'initial';
      } else if (pending.addType === 'plan') {
        nextScrollAction = 'plan';
      } else if (pending.addType === 'running') {
        nextScrollAction = 'running';
      } else {
        nextScrollAction = 'default';
      }

      const aggregatedEntries = aggregateConsecutiveEntries(pending.entries);

      // Filter out entries that render as null in the new design –
      // leaving them in creates empty Virtuoso items that add spacing.
      const filteredEntries = aggregatedEntries.filter((entry) => {
        if (
          'type' in entry &&
          entry.type === 'NORMALIZED_ENTRY' &&
          typeof entry.content !== 'string' &&
          'entry_type' in entry.content
        ) {
          const t = entry.content.entry_type.type;
          return t !== 'next_action' && t !== 'token_usage_info';
        }
        return true;
      });

      setDisplayEntries(filteredEntries);
      setPendingScrollAction(nextScrollAction);
      setEntries(pending.entries);

      if (loadingRef.current) {
        loadingRef.current = pending.loading;
        setLoading(pending.loading);
      }
    }, 100);
  };

  const {
    hasSetupScriptRun,
    hasCleanupScriptRun,
    hasRunningProcess,
    isFirstTurn,
  } = useConversationHistory({ attempt, onEntriesUpdated });

  // Determine if there are entries to show placeholders
  const hasEntries = displayEntries.length > 0;

  // Show placeholders only if script not configured AND not already run AND first turn
  const showSetupPlaceholder =
    !hasSetupScript && !hasSetupScriptRun && hasEntries;
  const showCleanupPlaceholder =
    !hasCleanupScript &&
    !hasCleanupScriptRun &&
    !hasRunningProcess &&
    hasEntries &&
    isFirstTurn;

  const messageListContext: MessageListContext = {
    attempt,
    onConfigureSetup: canConfigure ? handleConfigureSetup : undefined,
    onConfigureCleanup: canConfigure ? handleConfigureCleanup : undefined,
    showSetupPlaceholder,
    showCleanupPlaceholder,
    resetAction,
  };

  useEffect(() => {
    if (!pendingScrollAction || displayEntries.length === 0) {
      return;
    }

    const lastIndex = displayEntries.length - 1;
    const scroll = () => {
      if (!messageListRef.current) return;

      if (pendingScrollAction === 'plan') {
        messageListRef.current.scrollToIndex({
          index: lastIndex,
          align: 'start',
          behavior: 'smooth',
        });
      } else if (
        pendingScrollAction === 'initial' ||
        pendingScrollAction === 'default' ||
        (pendingScrollAction === 'running' && isAtBottomRef.current)
      ) {
        messageListRef.current.scrollToIndex({
          index: lastIndex,
          align: 'end',
          behavior: pendingScrollAction === 'running' ? 'smooth' : 'auto',
        });
      }
      setPendingScrollAction(null);
    };

    requestAnimationFrame(scroll);
    previousLengthRef.current = displayEntries.length;
  }, [displayEntries, pendingScrollAction]);

  // Expose scroll to previous user message functionality via ref
  useImperativeHandle(
    ref,
    () => ({
      scrollToPreviousUserMessage: () => {
        if (!displayEntries.length || !messageListRef.current) return;
        const firstVisibleIndex = Math.max(visibleStartIndexRef.current, 0);

        // Find all user message indices
        const userMessageIndices: Array<number> = [];
        displayEntries.forEach((item, index) => {
          if (
            item.type === 'NORMALIZED_ENTRY' &&
            item.content.entry_type.type === 'user_message'
          ) {
            userMessageIndices.push(index);
          }
        });

        // Find the user message before the first visible item
        const targetIndex = userMessageIndices
          .reverse()
          .find((idx) => idx < firstVisibleIndex);

        if (targetIndex !== undefined) {
          messageListRef.current.scrollToIndex({
            index: targetIndex,
            align: 'start',
            behavior: 'smooth',
          });
        }
      },
      scrollToBottom: () => {
        if (!messageListRef.current || displayEntries.length === 0) return;
        messageListRef.current.scrollToIndex({
          index: displayEntries.length - 1,
          align: 'end',
          behavior: 'smooth',
        });
      },
    }),
    [displayEntries]
  );

  // Determine if content is ready to show (has data or finished loading)
  const hasContent = !loading || displayEntries.length > 0;

  return (
    <ApprovalFormProvider>
      <div
        className={cn(
          'virtuoso-license-wrapper relative h-full overflow-hidden transition-opacity duration-300',
          hasContent ? 'opacity-100' : 'opacity-0'
        )}
      >
        {!hasContent && (
          <div className="absolute inset-0 flex items-center justify-center z-10">
            <SpinnerIcon className="size-6 animate-spin text-low" />
          </div>
        )}
        <Virtuoso<DisplayEntry>
          ref={messageListRef}
          className="h-full scrollbar-none"
          data={displayEntries}
          computeItemKey={(_, item) => `conv-${item.patchKey}`}
          atBottomStateChange={(isAtBottom) => {
            if (isAtBottom !== isAtBottomRef.current) {
              isAtBottomRef.current = isAtBottom;
              onAtBottomChange?.(isAtBottom);
            }
          }}
          rangeChanged={(range) => {
            visibleStartIndexRef.current = range.startIndex;
          }}
          itemContent={(_, item) => (
            <ItemContent data={item} context={messageListContext} />
          )}
          components={{
            Header: () => (
              <div className="pt-2">
                {messageListContext.showSetupPlaceholder && (
                  <div className="my-base px-double">
                    <ChatScriptPlaceholder
                      type="setup"
                      onConfigure={messageListContext.onConfigureSetup}
                    />
                  </div>
                )}
              </div>
            ),
            Footer: () => (
              <div className="pb-2">
                {messageListContext.showCleanupPlaceholder && (
                  <div className="my-base px-double">
                    <ChatScriptPlaceholder
                      type="cleanup"
                      onConfigure={messageListContext.onConfigureCleanup}
                    />
                  </div>
                )}
              </div>
            ),
          }}
        />
      </div>
    </ApprovalFormProvider>
  );
});

export default ConversationList;
