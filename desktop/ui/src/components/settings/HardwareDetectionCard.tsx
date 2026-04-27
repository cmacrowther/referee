import { formatEncoderBackend, formatExecutorKind, formatGpuVendor } from '@/lib/stream';
import type { Status } from '@/lib/types';

interface HardwareDetectionCardProps {
    status: Status;
}

function HardwareRow({ label, value }: { label: string; value: string }) {
    return (
        <div className="flex items-center justify-between">
            <span className="text-[11px] uppercase tracking-wider text-referee-muted">{label}</span>
            <span className="text-right text-[12px] font-medium text-white">{value}</span>
        </div>
    );
}

export function HardwareDetectionCard({ status }: HardwareDetectionCardProps) {
    return (
        <div className="rounded-lg border border-referee-border bg-referee-card p-3.5">
            <div className="mb-3 text-[10px] font-bold uppercase tracking-widest text-referee-muted">
                Hardware Detection
            </div>
            <div className="flex flex-col gap-3">
                <HardwareRow label="GPU" value={status.gpuName || 'No supported GPU detected'} />
                <HardwareRow label="Vendor" value={formatGpuVendor(status.gpuVendor)} />
                <HardwareRow label="Encoder" value={formatEncoderBackend(status.encoderBackend)} />
                <HardwareRow label="Processing" value={formatExecutorKind(status.selectedExecutor)} />
            </div>
        </div>
    );
}
