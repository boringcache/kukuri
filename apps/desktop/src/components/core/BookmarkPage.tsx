import { type ReactNode, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';

export const BOOKMARK_PAGE_SIZE = 20;

export function BookmarkPage<T>({
  items,
  children,
}: {
  items: T[];
  children: (items: T[]) => ReactNode;
}) {
  const { t } = useTranslation('common');
  const [page, setPage] = useState(0);
  const pageCount = Math.max(1, Math.ceil(items.length / BOOKMARK_PAGE_SIZE));
  const currentPage = Math.min(page, pageCount - 1);
  const first = currentPage * BOOKMARK_PAGE_SIZE;
  const pageItems = items.slice(first, first + BOOKMARK_PAGE_SIZE);

  return (
    <>
      {children(pageItems)}
      {pageCount > 1 ? (
        <nav className='bookmark-pagination' aria-label={t('fallbacks.bookmarkPages')}>
          <Button
            variant='secondary'
            type='button'
            disabled={currentPage === 0}
            onClick={() => setPage((value) => Math.max(0, value - 1))}
          >
            {t('actions.previousPage')}
          </Button>
          <span>{t('fallbacks.pageCount', { current: currentPage + 1, total: pageCount })}</span>
          <Button
            variant='secondary'
            type='button'
            disabled={currentPage + 1 >= pageCount}
            onClick={() => setPage((value) => Math.min(pageCount - 1, value + 1))}
          >
            {t('actions.nextPage')}
          </Button>
        </nav>
      ) : null}
    </>
  );
}
