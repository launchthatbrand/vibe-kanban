import { useMemo, useState } from 'react';
import { create, useModal } from '@ebay/nice-modal-react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@vibe/ui/components/KeyboardDialog';
import { Label } from '@vibe/ui/components/Label';
import { Button } from '@vibe/ui/components/Button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@vibe/ui/components/Select';
import { defineModal } from '@/shared/lib/modals';
import {
  type DevServerScriptEntry,
  getRepoDevServerScripts,
} from '@/shared/lib/devServerScripts';
import type { RepoWithTargetBranch } from 'shared/types';

interface RepoScriptSelection {
  repoId: string;
  repoName: string;
  scripts: DevServerScriptEntry[];
}

export interface DevServerSelectorDialogProps {
  repos: RepoWithTargetBranch[];
}

export interface DevServerSelectorDialogResult {
  confirmed: boolean;
  repoScriptIds?: Record<string, string>;
}

const DevServerSelectorDialogImpl = create<DevServerSelectorDialogProps>(
  ({ repos }) => {
    const modal = useModal();
    const { t } = useTranslation('common');

    const selectableRepos = useMemo<RepoScriptSelection[]>(() => {
      return repos
        .map((repo) => ({
          repoId: repo.id,
          repoName: repo.display_name || repo.name,
          scripts: getRepoDevServerScripts(repo),
        }))
        .filter((repo) => repo.scripts.length > 0);
    }, [repos]);

    const [selectedByRepo, setSelectedByRepo] = useState<Record<string, string>>(
      () =>
        Object.fromEntries(
          selectableRepos.map((repo) => [repo.repoId, repo.scripts[0]?.id ?? ''])
        )
    );

    const handleClose = () => {
      modal.resolve({ confirmed: false } as DevServerSelectorDialogResult);
      modal.hide();
    };

    const handleStart = () => {
      modal.resolve({
        confirmed: true,
        repoScriptIds: selectedByRepo,
      } as DevServerSelectorDialogResult);
      modal.hide();
    };

    if (selectableRepos.length === 0) {
      return null;
    }

    return (
      <Dialog open={modal.visible} onOpenChange={(open) => !open && handleClose()}>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>
              {t('attempt.actions.startDevServer')} - Select script
            </DialogTitle>
          </DialogHeader>

          <div className="flex flex-col gap-4">
            {selectableRepos.map((repo) => (
              <div key={repo.repoId} className="flex flex-col gap-2">
                <Label>{repo.repoName}</Label>
                <Select
                  value={selectedByRepo[repo.repoId] ?? repo.scripts[0]?.id ?? ''}
                  onValueChange={(value) =>
                    setSelectedByRepo((prev) => ({ ...prev, [repo.repoId]: value }))
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {repo.scripts.map((script) => (
                      <SelectItem key={script.id} value={script.id}>
                        {script.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ))}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={handleClose}>
              {t('buttons.cancel')}
            </Button>
            <Button onClick={handleStart}>{t('attempt.actions.startDevServer')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
);

export const DevServerSelectorDialog = defineModal<
  DevServerSelectorDialogProps,
  DevServerSelectorDialogResult
>(DevServerSelectorDialogImpl);
