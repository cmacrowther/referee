import { ArrowUpCircle, BookOpen, ExternalLink, GitBranch, PlayCircle, Scale, User } from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';

import { SettingsSection } from '@/components/settings/SettingsPrimitives';
import type { RendererApi, UpdateInfo } from '@/lib/types';

interface AboutCardProps {
    api: RendererApi;
}

interface LinkRowProps {
    icon: ReactNode;
    title: string;
    url: string;
    onClick: () => void;
}

function LinkRow({ icon, title, url, onClick }: LinkRowProps) {
    return (
        <button
            type="button"
            title={url}
            onClick={onClick}
            className="group flex w-full cursor-pointer items-center justify-between gap-2 px-3 py-2 transition-colors hover:bg-white/[0.02]"
        >
            <span className="flex min-w-0 items-center gap-2.5">
                <span className="shrink-0 text-referee-muted transition-colors group-hover:text-white">{icon}</span>
                <span className="truncate text-[12px] font-medium text-white">{title}</span>
            </span>
            <ExternalLink className="size-3 shrink-0 text-referee-muted transition-colors group-hover:text-white" />
        </button>
    );
}

export function AboutCard({ api }: AboutCardProps) {
    const [version, setVersion] = useState('...');
    const [updateInfo, setUpdateInfo] = useState<UpdateInfo>({
        currentVersion: '...',
        hasUpdate: false
    });
    const [downloadProgress, setDownloadProgress] = useState<number | null>(null);
    const [isInstalling, setIsInstalling] = useState(false);

    useEffect(() => {
        let isMounted = true;

        api.getAppVersion()
            .then(nextVersion => {
                if (isMounted) {
                    setVersion(nextVersion);
                }
            })
            .catch(() => {});

        api.checkForUpdate()
            .then(nextUpdateInfo => {
                if (isMounted) {
                    setUpdateInfo(nextUpdateInfo);
                    if (nextUpdateInfo.currentVersion) {
                        setVersion(nextUpdateInfo.currentVersion);
                    }
                }
            })
            .catch(() => {});

        const unsubscribe = api.onUpdateProgress(percent => {
            setDownloadProgress(percent);
        });

        return () => {
            isMounted = false;
            unsubscribe();
        };
    }, [api]);

    async function handleInstallUpdate() {
        if (!updateInfo.downloadUrl) {
            return;
        }

        setIsInstalling(true);
        try {
            await api.downloadAndInstallUpdate(updateInfo.downloadUrl);
        } catch {
            setIsInstalling(false);
            setDownloadProgress(null);
        }
    }

    const updateLabel = downloadProgress !== null ? `${downloadProgress}%` : 'Update';

    return (
        <SettingsSection
            title="About"
            aside={
                <div className="flex items-center gap-2">
                    <span className="text-[10px] font-medium text-referee-muted">v{version}</span>
                    {updateInfo.hasUpdate && updateInfo.downloadUrl && (
                        <button
                            type="button"
                            disabled={isInstalling}
                            onClick={handleInstallUpdate}
                            className="inline-flex items-center gap-1 rounded bg-referee-green/15 px-2 py-0.5 text-[9px] font-bold uppercase tracking-wider text-referee-green transition-colors hover:bg-referee-green/25 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                            <ArrowUpCircle className="size-[11px]" />
                            <span>{updateLabel}</span>
                        </button>
                    )}
                </div>
            }
        >
            <LinkRow
                icon={<BookOpen className="size-4" />}
                title="Documentation"
                url="https://referee.craw.ca"
                onClick={() => {
                    api.openExternal('https://referee.craw.ca/');
                }}
            />
            <LinkRow
                icon={<PlayCircle className="size-4" />}
                title="Demo"
                url="https://referee.craw.ca/demo"
                onClick={() => {
                    api.openExternal('https://referee.craw.ca/demo');
                }}
            />
            <LinkRow
                icon={<GitBranch className="size-4" />}
                title="Repository"
                url="https://github.com/cmacrowther/referee"
                onClick={() => {
                    api.openGithub();
                }}
            />
            <LinkRow
                icon={<Scale className="size-4" />}
                title="Licensing"
                url="https://github.com/cmacrowther/referee/blob/main/LICENSE"
                onClick={() => {
                    api.openExternal('https://github.com/cmacrowther/referee/blob/main/LICENSE');
                }}
            />
            <LinkRow
                icon={<User className="size-4" />}
                title="Colin Crowther"
                url="https://craw.ca"
                onClick={() => {
                    api.openExternal('https://craw.ca/');
                }}
            />
        </SettingsSection>
    );
}
