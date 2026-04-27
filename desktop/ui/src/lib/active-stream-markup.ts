import whistleMarkup from '../../../../assets/orange-whistle.svg?raw';

export const ACTIVE_STREAM_MARKUP = whistleMarkup
    .replace(/^<\?xml[^>]*>\s*/i, '')
    .replace(/<defs><bx:export>.*?<\/bx:export><\/defs>/i, '');

function normalizeSvgFillValue(fillValue: string) {
    const normalizedValue = (fillValue || '').trim().toLowerCase();
    if (!normalizedValue) {
        return '';
    }

    const rgbMatch = normalizedValue.match(/^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})/i);
    if (!rgbMatch) {
        return normalizedValue;
    }

    return `#${rgbMatch
        .slice(1, 4)
        .map(value => Math.max(0, Math.min(255, Number(value))).toString(16).padStart(2, '0'))
        .join('')}`;
}

function getSvgPathFillValue(pathElement: SVGPathElement) {
    const inlineStyleFill = normalizeSvgFillValue(pathElement.style?.fill || '');
    const attributeFill = normalizeSvgFillValue(pathElement.getAttribute('fill') || '');
    return inlineStyleFill || attributeFill;
}

export function decorateStreamMarkup(container: Element | null) {
    const whistleSvg = container?.querySelector('svg');
    if (!whistleSvg) {
        return;
    }

    let signalIndex = 0;
    Array.from(whistleSvg.querySelectorAll('path')).forEach(pathElement => {
        pathElement.removeAttribute('data-whistle-signal');
        pathElement.removeAttribute('data-whistle-tone');
        pathElement.style.removeProperty('--signal-index');
        pathElement.style.removeProperty('animation-delay');

        const fillValue = getSvgPathFillValue(pathElement);

        if (fillValue === '#ffffff') {
            pathElement.setAttribute('data-whistle-signal', 'true');
            pathElement.style.setProperty('--signal-index', String(signalIndex));
            pathElement.style.setProperty('animation-delay', `${signalIndex * 140}ms`);
            signalIndex += 1;
            return;
        }

        if (fillValue === '#3a3a3a') {
            pathElement.setAttribute('data-whistle-tone', 'core');
            return;
        }

        if (fillValue === '#100f0f') {
            pathElement.setAttribute('data-whistle-tone', 'detail');
            return;
        }

        pathElement.setAttribute('data-whistle-tone', 'shell');
    });
}
