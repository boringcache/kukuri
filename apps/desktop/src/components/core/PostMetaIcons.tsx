import type { LucideIcon } from 'lucide-react';
import { BookLock, BookOpen, UserRoundArrowLeft, UserRoundGroup, UsersRound } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ReactionTooltipButton } from './ReactionTooltipButton';
import type { PostAudienceView } from './types';

// #1345: 投稿カードのフォロー関係・公開範囲は icon で示し、意味は tooltip と accessible name で伝える。
// 状態表示であり操作ではないため tab 移動の対象にしない。
function PostMetaIcon({ icon: Icon, label }: { icon: LucideIcon; label: string }) {
  return (
    <ReactionTooltipButton label={label}>
      <span className='post-meta-icon' role='img' aria-label={label}>
        <Icon className='size-4' aria-hidden='true' />
      </span>
    </ReactionTooltipButton>
  );
}

export function PostMetaIcons({
  relationshipLabel,
  audience,
}: {
  relationshipLabel: string | null;
  audience: PostAudienceView;
}) {
  const { t } = useTranslation('common');
  const relationship =
    relationshipLabel === 'mutual'
      ? { icon: UsersRound, label: t('relationships.mutual') }
      : relationshipLabel === 'following'
        ? { icon: UserRoundArrowLeft, label: t('relationships.follow') }
        : relationshipLabel === 'follows you'
          ? { icon: UserRoundArrowLeft, label: t('relationships.follower') }
          : relationshipLabel === 'friend of friend'
            ? { icon: UserRoundGroup, label: t('relationships.viaFollow') }
            : null;

  return (
    <>
      {relationship ? <PostMetaIcon {...relationship} /> : null}
      {audience.kind === 'public' ? (
        <PostMetaIcon icon={BookOpen} label={t('audience.public')} />
      ) : (
        <PostMetaIcon
          icon={BookLock}
          label={
            audience.channelLabel
              ? t('audience.privateNamed', { channel: audience.channelLabel })
              : t('audience.private')
          }
        />
      )}
    </>
  );
}
