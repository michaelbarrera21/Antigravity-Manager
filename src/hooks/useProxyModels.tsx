import { useTranslation } from 'react-i18next';
import {
    Zap,
    Cpu,
    BrainCircuit,
    Sparkles
} from 'lucide-react';

export const useProxyModels = () => {
    const { t } = useTranslation();

    const models = [
        // Gemini 3 Series
        {
            id: 'gemini-3-flash',
            name: 'Gemini 3 Flash',
            desc: t('proxy.model.flash_preview'),
            group: 'Gemini 3',
            icon: <Zap size={16} />
        },
        {
            id: 'gemini-3.1-pro-high',
            name: 'Gemini 3.1 Pro High',
            desc: t('proxy.model.pro_high'),
            group: 'Gemini 3',
            icon: <Cpu size={16} />
        },

        // Gemini 2.5 Series
        {
            id: 'gemini-2.5-flash',
            name: 'Gemini 2.5 Flash',
            desc: t('proxy.model.flash'),
            group: 'Gemini 2.5',
            icon: <Zap size={16} />
        },
        {
            id: 'gemini-2.5-flash-lite',
            name: 'Gemini 2.5 Flash Lite',
            desc: t('proxy.model.flash_lite'),
            group: 'Gemini 2.5',
            icon: <Zap size={16} />
        },
        {
            id: 'gemini-2.5-flash-thinking',
            name: 'Gemini 2.5 Flash (Thinking)',
            desc: t('proxy.model.claude_sonnet_thinking'),
            group: 'Gemini 2.5',
            icon: <BrainCircuit size={16} />
        },

        // Claude Series
        {
            id: 'claude-sonnet-4-6',
            name: 'Claude 4.6 Sonnet',
            desc: t('proxy.model.claude_sonnet'),
            group: 'Claude 4.6',
            icon: <Sparkles size={16} />
        },
        {
            id: 'claude-opus-4-5-thinking',
            name: 'Claude 4.5 Opus (Thinking)',
            desc: t('proxy.model.claude_opus_thinking'),
            group: 'Claude 4.5',
            icon: <Cpu size={16} />
        }
    ];

    return { models };
};
