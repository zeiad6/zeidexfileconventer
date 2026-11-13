import { createRoot } from 'react-dom/client';
import { Editor } from '../editor/Editor';
import { I18nProvider } from '../lib/i18n';
import '../styles.css';

createRoot(document.getElementById('root')!).render(
  <I18nProvider>
    <Editor />
  </I18nProvider>,
);
