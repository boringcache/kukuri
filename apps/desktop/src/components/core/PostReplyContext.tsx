import type { KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';

import type { InternalSmartReference } from '@/lib/internalLinks';
import type { PostView } from '@/lib/api';

import { AuthorAvatar } from './AuthorAvatar';
import { MediaFetchFailure } from './MediaFetchFailure';
import { SmartReferenceText } from './SmartReferenceText';
import type { MentionAuthorView, ReferencedAuthorMeta } from './types';

export function PostReplyContext({
  canOpenThread,
  mentionAuthors,
  onActivateReference,
  onOpenAuthor,
  onOpenThread,
  onRetryBody,
  parentAuthor,
  readOnly,
  reloadPending,
  replyPreview,
}: {
  canOpenThread: boolean;
  mentionAuthors?: Record<string, MentionAuthorView>;
  onActivateReference?: (reference: InternalSmartReference) => void;
  onOpenAuthor: (pubkey: string) => void;
  onOpenThread: () => void;
  onRetryBody?: () => void;
  parentAuthor: ReferencedAuthorMeta;
  readOnly: boolean;
  reloadPending: boolean;
  replyPreview: NonNullable<PostView['reply_preview']>;
}) {
  const { t } = useTranslation('common');
  const openFromKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (readOnly || !canOpenThread || event.target !== event.currentTarget) return;
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onOpenThread();
    }
  };

  return (
    <div className='post-reply-context'>
      <button
        type='button'
        className='post-reply-context-avatar'
        aria-label={parentAuthor.label}
        onClick={(event) => {
          event.stopPropagation();
          onOpenAuthor(parentAuthor.pubkey);
        }}
      >
        <AuthorAvatar label={parentAuthor.label} picture={parentAuthor.picture ?? null} size='sm' />
      </button>
      <div className='post-reply-context-main'>
        <button
          type='button'
          className='post-reply-context-author author-link'
          onClick={(event) => {
            event.stopPropagation();
            onOpenAuthor(parentAuthor.pubkey);
          }}
        >
          {t('feed.replyingTo', { author: parentAuthor.label })}
        </button>
        {replyPreview.content_status === 'Missing' ? (
          <MediaFetchFailure
            hashes={[]}
            retrying={reloadPending}
            onRetry={onRetryBody}
            testId={`reply-body-fetch-failure-${replyPreview.object_id}`}
          />
        ) : replyPreview.content.trim().length > 0 ? (
          <div
            className='post-reply-context-body post-copy-wrap'
            role={!readOnly && canOpenThread ? 'button' : undefined}
            tabIndex={!readOnly && canOpenThread ? 0 : undefined}
            onClick={!readOnly && canOpenThread ? onOpenThread : undefined}
            onKeyDown={openFromKeyboard}
          >
            <SmartReferenceText
              text={replyPreview.content}
              className='post-copy-wrap'
              onActivateReference={onActivateReference}
              mentionAuthors={mentionAuthors}
              onOpenMention={onOpenAuthor}
              externalLinks
            />
          </div>
        ) : replyPreview.attachments.length > 0 ? (
          <span className='post-reply-context-body'>
            {t('feed.moreMedia', { count: replyPreview.attachments.length })}
          </span>
        ) : null}
      </div>
    </div>
  );
}
