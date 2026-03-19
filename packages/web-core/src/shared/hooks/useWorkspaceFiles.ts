import { useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { workspaceFilesApi } from '@/shared/lib/api';
import type {
  WorkspaceFileContentResponse,
  WorkspaceFileContentUpdateRequest,
  WorkspaceFileContentUpdateResponse,
  WorkspaceFileTreeResponse,
} from 'shared/types';

export const workspaceFilesKeys = {
  tree: (workspaceId: string | undefined, repoId: string | null, path: string) =>
    ['workspaceFilesTree', workspaceId, repoId, path] as const,
  content: (workspaceId: string | undefined, repoId: string | null, path: string | null) =>
    ['workspaceFileContent', workspaceId, repoId, path] as const,
};

export function useWorkspaceFileTree(
  workspaceId: string | undefined,
  repoId: string | null,
  path = '',
  enabled = true
) {
  const query = useQuery<WorkspaceFileTreeResponse>({
    queryKey: workspaceFilesKeys.tree(workspaceId, repoId, path),
    queryFn: () =>
      workspaceFilesApi.getTree(workspaceId!, {
        repo_id: repoId,
        path: path || null,
        recursive: true,
      }),
    enabled: enabled && !!workspaceId && !!repoId,
  });

  return useMemo(
    () => ({
      entries: query.data?.entries ?? [],
      currentPath: query.data?.current_path ?? '',
      isLoading: query.isLoading,
      isFetching: query.isFetching,
      error: query.error,
      refetch: query.refetch,
    }),
    [query.data, query.error, query.isFetching, query.isLoading, query.refetch]
  );
}

export function useWorkspaceFileContent(
  workspaceId: string | undefined,
  repoId: string | null,
  filePath: string | null,
  enabled = true
) {
  return useQuery<WorkspaceFileContentResponse>({
    queryKey: workspaceFilesKeys.content(workspaceId, repoId, filePath),
    queryFn: () =>
      workspaceFilesApi.getContent(workspaceId!, {
        repo_id: repoId,
        path: filePath!,
      }),
    enabled: enabled && !!workspaceId && !!repoId && !!filePath,
  });
}

export function useUpdateWorkspaceFileContent(workspaceId: string | undefined) {
  const queryClient = useQueryClient();

  return useMutation<
    WorkspaceFileContentUpdateResponse,
    Error,
    WorkspaceFileContentUpdateRequest
  >({
    mutationFn: (payload) => workspaceFilesApi.updateContent(workspaceId!, payload),
    onSuccess: (_result, variables) => {
      queryClient.invalidateQueries({
        queryKey: workspaceFilesKeys.content(
          workspaceId,
          variables.repo_id ?? null,
          variables.path
        ),
      });
      queryClient.invalidateQueries({
        queryKey: ['workspaceFilesTree', workspaceId],
      });
    },
  });
}
