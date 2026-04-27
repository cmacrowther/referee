import { useMemo } from 'react';

import { getRendererApi } from '@/lib/renderer-api';

export function useRendererApi() {
    return useMemo(() => getRendererApi(), []);
}
