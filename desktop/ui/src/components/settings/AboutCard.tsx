import { ArrowUpCircle, BookOpen, ExternalLink, GitBranch, PlayCircle, Scale, User } from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';

import type { RendererApi, UpdateInfo } from '@/lib/types';

interface AboutCardProps {
    api: RendererApi;
}

interface LinkRowProps {
    icon: ReactNode;
    title: string;
    subtitle: string;
    onClick: () => void;
}

function LinkRow({ icon, title, subtitle, onClick }: LinkRowProps) {
    return (
        <button
            type="button"
            onClick={onClick}
            className="group flex w-full cursor-pointer items-center justify-between border-t border-white/[0.04] px-3.5 py-3 transition-colors hover:bg-white/[0.02]"
        >
            <div className="flex min-w-0 items-center gap-2.5">
                <div className="shrink-0 text-referee-muted transition-colors group-hover:text-white">{icon}</div>
                <div className="flex min-w-0 flex-col text-left">
                    <span className="text-[13px] font-medium text-white">{title}</span>
                    <span className="truncate text-[11px] text-referee-muted">{subtitle}</span>
                </div>
            </div>
            <ExternalLink className="ml-2 size-3 shrink-0 text-referee-muted" />
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
        <div className="overflow-hidden rounded-lg border border-referee-border bg-referee-card">
            <div className="flex items-center justify-between px-3.5 pb-3 pt-3.5">
                <div className="text-[10px] font-bold uppercase tracking-widest text-referee-muted">About</div>
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
            </div>

            <LinkRow
                icon={<BookOpen className="size-4" />}
                title="Documentation"
                subtitle="https://referee.craw.ca"
                onClick={() => {
                    api.openExternal('https://referee.craw.ca/');
                }}
            />
            <LinkRow
                icon={<PlayCircle className="size-4" />}
                title="Demo"
                subtitle="https://referee.craw.ca/demo"
                onClick={() => {
                    api.openExternal('https://referee.craw.ca/demo');
                }}
            />
            <LinkRow
                icon={<GitBranch className="size-4" />}
                title="Repository"
                subtitle="https://github.com/cmacrowther/referee"
                onClick={() => {
                    api.openGithub();
                }}
            />
            <LinkRow
                icon={<Scale className="size-4" />}
                title="Licensing"
                subtitle="https://github.com/cmacrowther/referee/blob/main/LICENSE"
                onClick={() => {
                    api.openExternal('https://github.com/cmacrowther/referee/blob/main/LICENSE');
                }}
            />
            <LinkRow
                icon={<User className="size-4" />}
                title="Colin Crowther"
                subtitle="https://craw.ca"
                onClick={() => {
                    api.openExternal('https://craw.ca/');
                }}
            />
        </div>
    );
}
