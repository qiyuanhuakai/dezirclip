import type { UpdateChannel } from './updatePolicy';

/** Follow the installed release family until the user explicitly selects a channel. */
export function defaultUpdateChannel(version: string, savedChannel?: string): UpdateChannel {
    if (savedChannel === 'stable' || savedChannel === 'preview') return savedChannel;
    return /^\d+\.\d+\.\d+-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*(?:\+[0-9A-Za-z.-]+)?$/.test(version)
        ? 'preview' : 'stable';
}
