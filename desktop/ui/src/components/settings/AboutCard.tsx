import { BookOpen, ExternalLink, GitBranch, PlayCircle, Scale, User } from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';

import { SettingsSection } from '@/components/settings/SettingsPrimitives';
import type { RendererApi } from '@/lib/types';

interface AboutCardProps {
    api: RendererApi;
}

interface LinkRowProps {
    icon: ReactNode;
    title: string;
    url: string;
    onClick: () => void;
}

export function LinkRow({ icon, title, url, onClick }: LinkRowProps) {
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

    useEffect(() => {
        let isMounted = true;

        api.getAppVersion()
            .then(nextVersion => {
                if (isMounted) {
                    setVersion(nextVersion);
                }
            })
            .catch(() => {});

        return () => {
            isMounted = false;
        };
    }, [api]);

    return (
        <SettingsSection
            title="About"
            aside={<span className="text-[10px] font-medium text-referee-muted">v{version}</span>}
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
