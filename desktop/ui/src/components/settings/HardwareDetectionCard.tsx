import { SettingsSection } from '@/components/settings/SettingsPrimitives';
import { formatEncoderBackend, formatExecutorKind, formatGpuVendor } from '@/lib/stream';
import type { Status } from '@/lib/types';

interface HardwareDetectionCardProps {
    status: Status;
}

function HardwareRow({ label, value }: { label: string; value: string }) {
    return (
        <div className="flex items-center justify-between gap-3 px-3 py-2">
            <span className="shrink-0 text-[12px] text-referee-muted">{label}</span>
            <span title={value} className="min-w-0 truncate text-right text-[12px] font-medium text-white">{value}</span>
        </div>
    );
}

export function HardwareDetectionCard({ status }: HardwareDetectionCardProps) {
    return (
        <SettingsSection title="Hardware Detection">
            <HardwareRow label="GPU" value={status.gpuName || 'No supported GPU detected'} />
            <HardwareRow label="Vendor" value={formatGpuVendor(status.gpuVendor)} />
            <HardwareRow label="Encoder" value={formatEncoderBackend(status.encoderBackend)} />
            <HardwareRow label="Processing" value={formatExecutorKind(status.selectedExecutor)} />
        </SettingsSection>
    );
}
