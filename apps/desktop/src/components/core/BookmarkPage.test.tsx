import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from 'vitest';

import { BookmarkPage, BOOKMARK_PAGE_SIZE } from './BookmarkPage';

test('bookmark pagination replaces the bounded page instead of accumulating cards', async () => {
  const user = userEvent.setup();
  const items = Array.from({ length: BOOKMARK_PAGE_SIZE + 5 }, (_, index) => `bookmark-${index}`);
  render(
    <BookmarkPage items={items}>
      {(page) => page.map((item) => <div key={item}>{item}</div>)}
    </BookmarkPage>
  );

  expect(screen.getAllByText(/^bookmark-/)).toHaveLength(BOOKMARK_PAGE_SIZE);
  expect(screen.getByText('bookmark-0')).toBeInTheDocument();
  expect(screen.queryByText(`bookmark-${BOOKMARK_PAGE_SIZE}`)).not.toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: 'Next page' }));

  expect(screen.getAllByText(/^bookmark-/)).toHaveLength(5);
  expect(screen.queryByText('bookmark-0')).not.toBeInTheDocument();
  expect(screen.getByText(`bookmark-${BOOKMARK_PAGE_SIZE}`)).toBeInTheDocument();
});
