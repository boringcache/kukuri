import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from 'vitest';

import { PostCard } from './PostCard';
import { createView } from './PostCard.testHelpers';

// #1345: フォロー関係・公開範囲は文字チップではなく icon と tooltip で示す。
test.each([
  ['mutual', 'users-round', 'mutual follow'],
  ['following', 'user-round-arrow-left', 'following'],
  ['follows you', 'user-round-arrow-left', 'follower'],
  ['friend of friend', 'user-round-group', 'connected via someone you follow'],
])('post card shows the %s relationship as an icon with a tooltip', async (label, icon, name) => {
  const user = userEvent.setup();
  render(
    <PostCard
      view={{ ...createView(), relationshipLabel: label }}
      onOpenAuthor={() => undefined}
      onOpenThread={() => undefined}
      onReply={() => undefined}
    />
  );

  const relationshipIcon = screen.getByRole('img', { name });
  expect(relationshipIcon.querySelector(`.lucide-${icon}`)).not.toBeNull();
  expect(relationshipIcon).not.toHaveAttribute('tabindex');
  await user.hover(relationshipIcon);
  expect(await screen.findByRole('tooltip')).toHaveTextContent(name);
});

test.each([
  [{ kind: 'public' }, 'book-open', 'Public'],
  [{ kind: 'private', channelLabel: 'core contributors' }, 'book-lock', 'Private: core contributors'],
  [{ kind: 'private', channelLabel: null }, 'book-lock', 'Private'],
] as const)('post card shows the audience %o as an icon with a tooltip', async (audience, icon, name) => {
  const user = userEvent.setup();
  render(
    <PostCard
      view={{ ...createView(), audience }}
      onOpenAuthor={() => undefined}
      onOpenThread={() => undefined}
      onReply={() => undefined}
    />
  );

  expect(screen.queryByText('core contributors')).not.toBeInTheDocument();
  const audienceIcon = screen.getByRole('img', { name });
  expect(audienceIcon.querySelector(`.lucide-${icon}`)).not.toBeNull();
  await user.hover(audienceIcon);
  expect(await screen.findByRole('tooltip')).toHaveTextContent(name);
});

test('post card shows no relationship icon without a relationship', () => {
  const { container } = render(
    <PostCard
      view={createView()}
      onOpenAuthor={() => undefined}
      onOpenThread={() => undefined}
      onReply={() => undefined}
    />
  );

  expect(container.querySelectorAll('.post-meta-icon')).toHaveLength(1);
});
