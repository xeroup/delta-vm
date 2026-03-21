class DocViewer {
    constructor() {
        this.docsPath = 'documentation/';
        this.init();
    }

    async init() {
        const structure = await this.buildStructureFromFiles();
        this.renderTree(structure);
        this.setupEvents();
        this.loadFromHash();
    }

    async buildStructureFromFiles() {
        try {
            const response = await fetch(`files.json`);
            const files = await response.json();
            const structure = {};

            for (const file of files) {
                if (file.endsWith('.md')) {
                    const parts = file.split('/');
                    let current = structure;

                    for (let i = 0; i < parts.length; i++) {
                        const part = parts[i];
                        const isLast = i === parts.length - 1;

                        if (isLast) {
                            current[part] = part.replace(/\.md$/, '');
                        } else {
                            if (!current[part]) {
                                current[part] = {};
                            }
                            current = current[part];
                        }
                    }
                }
            }
            return structure;
        } catch {
            return {};
        }
    }

    renderTree(structure) {
        const container = document.getElementById('doc-tree');
        container.innerHTML = '';

        const renderItems = (items, path = '', parentElement = container) => {
            const keys = Object.keys(items);

            for (const key of keys) {
                const value = items[key];
                const isFolder = typeof value === 'object';
                const fullPath = path ? `${path}/${key}` : key;
                const displayName = isFolder ? key : value;

                const item = document.createElement('div');
                item.className = `tree-item ${isFolder ? 'folder' : 'file'}`;
                if (!isFolder) item.dataset.path = fullPath;

                const icon = document.createElement('span');
                icon.className = 'tree-icon';

                const text = document.createElement('span');
                text.className = 'tree-text';
                text.textContent = displayName;

                item.appendChild(icon);
                item.appendChild(text);
                parentElement.appendChild(item);

                if (isFolder) {
                    const arrow = document.createElement('span');
                    arrow.className = 'folder-arrow';
                    arrow.textContent = '[+]';
                    icon.appendChild(arrow);

                    const childDiv = document.createElement('div');
                    childDiv.className = 'tree-children';
                    childDiv.style.display = 'none';

                    const toggle = (e) => {
                        e.stopPropagation();
                        const hidden = childDiv.style.display === 'none';
                        childDiv.style.display = hidden ? 'block' : 'none';
                        arrow.textContent = hidden ? '[-]' : '[+]';
                    };

                    icon.onclick = toggle;
                    text.onclick = toggle;

                    renderItems(value, fullPath, childDiv);
                    parentElement.appendChild(childDiv);
                } else {
                    icon.textContent = '';
                    item.onclick = (e) => {
                        if (e.target !== icon && !e.target.classList.contains('folder-arrow')) {
                            this.loadFile(fullPath);
                        }
                    };
                }
            }
        };

        renderItems(structure);
    }

    async loadFile(path) {
        document.querySelectorAll('.tree-item.active').forEach(i => {
            i.classList.remove('active');
        });

        const activeItem = document.querySelector(`[data-path="${path}"]`);
        if (activeItem) {
            activeItem.classList.add('active');

            let parent = activeItem.parentElement;
            while (parent && parent.classList.contains('tree-children')) {
                const folder = parent.previousElementSibling;
                if (folder && folder.classList.contains('folder')) {
                    const arrow = folder.querySelector('.folder-arrow');
                    if (arrow) {
                        parent.style.display = 'block';
                        arrow.textContent = '[-]';
                    }
                }
                parent = parent.parentElement;
            }
        }

        const container = document.getElementById('markdown-container');
        container.innerHTML = '<div class="loading">loading...</div>';

        try {
            const response = await fetch(`${this.docsPath}${path}`);
            const text = await response.text();
            const html = marked.parse(text);

            container.innerHTML = html;
            window.location.hash = path;

            if (window.hljs) {
                document.querySelectorAll('pre code').forEach(block => {
                    hljs.highlightElement(block);
                });
            }

        } catch {
            container.innerHTML = '<div class="error">failed to load</div>';
        }
    }

    setupEvents() {
        window.addEventListener('hashchange', () => this.loadFromHash());
    }

    loadFromHash() {
        const hash = window.location.hash.substring(1);
        if (hash && hash.endsWith('.md')) {
            this.loadFile(hash);
        }
    }
}

document.addEventListener('DOMContentLoaded', () => {
    window.viewer = new DocViewer();
});