import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useWorkspacePanelState } from '@/shared/stores/useUiPreferencesStore';
import { useWorkspaceRepo } from '@/shared/hooks/useWorkspaceRepo';
import {
  useWorkspaceFileContent,
  useUpdateWorkspaceFileContent,
} from '@/shared/hooks/useWorkspaceFiles';
import { useOpenInEditor } from '@/shared/hooks/useOpenInEditor';
import { cn } from '@/shared/lib/utils';

interface FileEditorPanelContainerProps {
  workspaceId: string;
  className?: string;
}

export function FileEditorPanelContainer({
  workspaceId,
  className,
}: FileEditorPanelContainerProps) {
  const { t } = useTranslation('common');
  const { selectedAllFilePath } = useWorkspacePanelState(workspaceId);
  const { selectedRepoId } = useWorkspaceRepo(workspaceId);
  const openInEditor = useOpenInEditor(workspaceId);
  const [draftContent, setDraftContent] = useState('');
  const [lastSavedContent, setLastSavedContent] = useState('');

  const {
    data: fileContent,
    isLoading,
    isFetching,
    error,
    refetch,
  } = useWorkspaceFileContent(
    workspaceId,
    selectedRepoId,
    selectedAllFilePath,
    !!selectedAllFilePath
  );

  const updateFileMutation = useUpdateWorkspaceFileContent(workspaceId);

  useEffect(() => {
    const nextContent = fileContent?.content ?? '';
    setDraftContent(nextContent);
    setLastSavedContent(nextContent);
  }, [fileContent?.content, selectedAllFilePath]);

  const isDirty = draftContent !== lastSavedContent;
  const isBinary = fileContent?.is_binary ?? false;
  const isTooLarge = fileContent?.is_too_large ?? false;
  const isReadOnly = isBinary || isTooLarge;

  const handleSave = useCallback(async () => {
    if (!selectedAllFilePath || !selectedRepoId || isReadOnly) return;
    await updateFileMutation.mutateAsync({
      repo_id: selectedRepoId,
      path: selectedAllFilePath,
      content: draftContent,
      expected_modified_at_ms: fileContent?.modified_at_ms ?? null,
    });
    setLastSavedContent(draftContent);
  }, [
    selectedAllFilePath,
    selectedRepoId,
    isReadOnly,
    updateFileMutation,
    draftContent,
    fileContent?.modified_at_ms,
  ]);

  const handleOpenInIde = useCallback(() => {
    if (!selectedAllFilePath) return;
    void openInEditor({ filePath: selectedAllFilePath });
  }, [openInEditor, selectedAllFilePath]);

  const fileStatus = useMemo(() => {
    if (!selectedAllFilePath) return t('fileEditor.noFileSelected');
    if (isLoading || isFetching) return t('states.loading');
    if (error) return t('fileEditor.loadError');
    if (isBinary) return t('fileEditor.binaryUnsupported');
    if (isTooLarge) return t('fileEditor.fileTooLarge');
    return selectedAllFilePath;
  }, [selectedAllFilePath, isLoading, isFetching, error, isBinary, isTooLarge, t]);

  return (
    <div className={cn('h-full w-full bg-secondary flex flex-col', className)}>
      <div className="flex items-center justify-between gap-base px-base py-half border-b border-border">
        <div className="min-w-0">
          <p className="text-xs text-low">{t('sections.files')}</p>
          <p className="text-sm text-normal truncate">{fileStatus}</p>
        </div>
        <div className="flex items-center gap-half">
          <button
            type="button"
            onClick={() => {
              void refetch();
            }}
            className="px-2 py-1 text-xs rounded border border-border text-low hover:text-normal"
            disabled={!selectedAllFilePath || isLoading}
          >
            {t('actions.refresh')}
          </button>
          <button
            type="button"
            onClick={handleOpenInIde}
            className="px-2 py-1 text-xs rounded border border-border text-low hover:text-normal"
            disabled={!selectedAllFilePath}
          >
            {t('actions.openInIde')}
          </button>
          <button
            type="button"
            onClick={() => {
              setDraftContent(lastSavedContent);
            }}
            className="px-2 py-1 text-xs rounded border border-border text-low hover:text-normal"
            disabled={!isDirty || isReadOnly}
          >
            {t('buttons.discard')}
          </button>
          <button
            type="button"
            onClick={() => {
              void handleSave();
            }}
            className={cn(
              'px-2 py-1 text-xs rounded border transition-colors',
              isDirty && !isReadOnly
                ? 'border-brand text-brand hover:bg-brand/10'
                : 'border-border text-low'
            )}
            disabled={!isDirty || isReadOnly || updateFileMutation.isPending}
          >
            {updateFileMutation.isPending ? t('states.saving') : t('buttons.save')}
          </button>
        </div>
      </div>

      {!selectedAllFilePath ? (
        <div className="flex-1 min-h-0 flex items-center justify-center text-low text-sm">
          {t('fileEditor.selectFilePrompt')}
        </div>
      ) : isReadOnly ? (
        <div className="flex-1 min-h-0 flex items-center justify-center text-low text-sm px-base text-center">
          {isBinary ? t('fileEditor.binaryUnsupported') : t('fileEditor.fileTooLarge')}
        </div>
      ) : (
        <textarea
          value={draftContent}
          onChange={(event) => setDraftContent(event.target.value)}
          className="flex-1 min-h-0 w-full resize-none p-base bg-primary text-normal font-mono text-sm outline-none"
          spellCheck={false}
        />
      )}
    </div>
  );
}
