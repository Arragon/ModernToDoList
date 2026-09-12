const { createApp, ref, reactive, computed, onMounted, watch, toRaw } = Vue;

const DB_NAME = 'ModernToDoListDB';
const STORE_NAME = 'workspaces';

function openDB() {
    return new Promise((resolve, reject) => {
        const req = indexedDB.open(DB_NAME, 1);
        req.onupgradeneeded = (e) => {
            e.target.result.createObjectStore(STORE_NAME);
        };
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
    });
}

async function dbGet(key) {
    try {
        const db = await openDB();
        return new Promise((resolve, reject) => {
            const tx = db.transaction(STORE_NAME, 'readonly');
            const store = tx.objectStore(STORE_NAME);
            const req = store.get(key);
            req.onsuccess = () => resolve(req.result);
            req.onerror = () => reject(req.error);
        });
    } catch (e) {
        console.error("DB Get Error", e);
        return null;
    }
}

async function dbSet(key, value) {
    try {
        const db = await openDB();
        return new Promise((resolve, reject) => {
            const tx = db.transaction(STORE_NAME, 'readwrite');
            const store = tx.objectStore(STORE_NAME);
            const req = store.put(value, key);
            req.onsuccess = () => resolve();
            req.onerror = () => reject(req.error);
        });
    } catch (e) {
        console.error("DB Set Error", e);
    }
}

function debounce(fn, delay) {
    let timeout;
    return function(...args) {
        clearTimeout(timeout);
        timeout = setTimeout(() => fn.apply(this, args), delay);
    };
}

// Global drag state
const dragState = reactive({
    draggedTask: null,
    dragOverTaskId: null
});

// Recursive component for rendering tasks
const TaskList = {
    name: 'TaskList',
    template: '#task-list-template',
    props: {
        tasks: {
            type: Array,
            required: true
        },
        selectedId: {
            type: String,
            default: null
        },
        disableDrag: {
            type: Boolean,
            default: false
        }
    },
    emits: ['select', 'add-subtask', 'delete-task', 'drag-start', 'drag-over', 'drop', 'open-file'],
    methods: {
        handleDragStart(event, task) {
            this.$emit('drag-start', { event, task });
        },
        handleDragOver(event, task) {
            this.$emit('drag-over', { event, task });
        },
        handleDrop(event, task) {
            this.$emit('drop', { event, task });
        },
        isDragOver(task) {
            return dragState.dragOverTaskId === task.id;
        },
        toggleComplete(task) {
            const newStatus = task.percentDone >= 100 ? 0 : 100;
            task.percentDone = newStatus;
            
            // Auto complete/uncomplete subtasks
            if (newStatus === 100) {
                const completeSubtasks = (t) => {
                    t.percentDone = 100;
                    if (t.children) {
                        t.children.forEach(completeSubtasks);
                    }
                };
                if (task.children) {
                    task.children.forEach(completeSubtasks);
                }
            }
        },
        getPriorityColor(priority) {
            const p = parseInt(priority) || 0;
            if (p >= 8) return 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.5)]'; // High priority glow
            if (p >= 5) return 'bg-yellow-500';
            if (p >= 1) return 'bg-blue-500';
            return 'bg-gray-200';
        },
        getDueDateStatus(dateString) {
            if (!dateString) return 'none';
            const dueDate = new Date(dateString);
            const now = new Date();
            
            // Set times to midnight for accurate day comparison
            const dueDay = new Date(dueDate.getFullYear(), dueDate.getMonth(), dueDate.getDate());
            const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
            
            const diffTime = dueDay - today;
            const diffDays = Math.ceil(diffTime / (1000 * 60 * 60 * 24));
            
            if (diffDays < 0) return 'overdue';
            if (diffDays === 0 || diffDays === 1) return 'warning'; // Today or Tomorrow
            return 'normal';
        },
        getDueDateClass(task) {
            if (task.percentDone >= 100) return 'text-gray-400 bg-gray-50'; // Completed
            
            const status = this.getDueDateStatus(task.dueDate);
            if (status === 'overdue') return 'text-red-600 bg-red-50 font-medium border border-red-100';
            if (status === 'warning') return 'text-amber-600 bg-amber-50 font-medium border border-amber-100';
            return 'text-gray-500 bg-gray-50';
        },
        getDueDateWarning(task) {
            if (task.percentDone >= 100) return false;
            const status = this.getDueDateStatus(task.dueDate);
            return status === 'overdue' || status === 'warning';
        },
        isOverdue(dateString) {
            if (!dateString) return false;
            const dueDate = new Date(dateString);
            const today = new Date();
            today.setHours(0, 0, 0, 0);
            return dueDate < today;
        }
    }
};

const app = createApp({
    components: {
        'task-list': TaskList
    },
    setup() {
        const todoLists = ref([]);
        const activeListId = ref(null);

        const activeList = computed(() => todoLists.value.find(l => l.id === activeListId.value));
        const tasks = computed(() => activeList.value ? activeList.value.tasks : []);
        const xmlDoc = computed(() => activeList.value ? activeList.value.xmlDoc : null);
        const fileHandle = computed(() => activeList.value ? activeList.value.fileHandle : null);
        const originalFileName = computed(() => activeList.value ? activeList.value.originalFileName : '');
        
        // Selected Task State
        const selectedTask = ref(null);

        // Confirmation Modal State
        const confirmModal = reactive({
            isOpen: false,
            title: '',
            message: '',
            onConfirm: null
        });

        const showConfirm = (title, message, onConfirm) => {
            confirmModal.title = title;
            confirmModal.message = message;
            confirmModal.onConfirm = async () => {
                try {
                    await onConfirm?.();
                } finally {
                    closeConfirm();
                }
            };
            confirmModal.isOpen = true;
        };

        const closeConfirm = () => {
            confirmModal.isOpen = false;
            confirmModal.title = '';
            confirmModal.message = '';
            confirmModal.onConfirm = null;
        };

        const toast = reactive({
            isOpen: false,
            message: '',
            type: 'success'
        });
        let toastTimer = null;

        const showToast = (message, type = 'success', duration = 2200) => {
            if (toastTimer) clearTimeout(toastTimer);
            toast.message = message;
            toast.type = type;
            toast.isOpen = true;
            toastTimer = setTimeout(() => {
                toast.isOpen = false;
            }, duration);
        };

        const closeToast = () => {
            if (toastTimer) {
                clearTimeout(toastTimer);
                toastTimer = null;
            }
            toast.isOpen = false;
            toast.message = '';
            toast.type = 'success';
        };

        watch(activeListId, () => {
            // When switching lists, restore the selected task for the new list if it exists
            if (activeList.value && activeList.value.selectedTaskId) {
                // We need a helper to find task by id
                const findTask = (tasks, id) => {
                    for (const t of tasks) {
                        if (t.id === id) return t;
                        if (t.children) {
                            const found = findTask(t.children, id);
                            if (found) return found;
                        }
                    }
                    return null;
                };
                selectedTask.value = findTask(activeList.value.tasks, activeList.value.selectedTaskId);
            } else {
                selectedTask.value = null;
            }
        });

        // Select Task Logic
        const selectTask = (task) => {
            selectedTask.value = task;
            if (activeList.value) {
                activeList.value.selectedTaskId = task ? task.id : null;
            }
        };
        const newTagInput = ref('');

        // Filter State
        const filterState = reactive({
            keyword: '',
            tag: '',
            dueDate: 'all', // all, today, tomorrow, this_week, next_week, overdue, no_date
            globalSearch: false
        });

        const isFiltering = computed(() => {
            return filterState.keyword !== '' || filterState.tag !== '' || filterState.dueDate !== 'all';
        });

        const hasActiveFilters = computed(() => {
            return isFiltering.value || filterState.globalSearch;
        });

        const clearFilters = () => {
            filterState.keyword = '';
            filterState.tag = '';
            filterState.dueDate = 'all';
            filterState.globalSearch = false;
        };

        // Extract all unique tags for the filter dropdown
        const allAvailableTags = computed(() => {
            const tags = new Set();
            const extractTags = (list) => {
                list.forEach(task => {
                    if (task.categories) {
                        task.categories.forEach(c => tags.add(c));
                    }
                    if (task.children) {
                        extractTags(task.children);
                    }
                });
            };
            if (filterState.globalSearch) {
                todoLists.value.forEach(list => extractTags(list.tasks));
            } else if (activeList.value) {
                extractTags(activeList.value.tasks);
            }
            return Array.from(tags).sort();
        });

        // Filter Logic
        const checkTaskMatch = (task) => {
            let match = true;

            // 1. Keyword search (title or comments)
            if (filterState.keyword) {
                const kw = filterState.keyword.toLowerCase();
                const titleMatch = task.title && task.title.toLowerCase().includes(kw);
                const commentsMatch = task.comments && task.comments.toLowerCase().includes(kw);
                if (!titleMatch && !commentsMatch) {
                    match = false;
                }
            }

            // 2. Tag filter
            if (match && filterState.tag) {
                if (!task.categories || !task.categories.includes(filterState.tag)) {
                    match = false;
                }
            }

            // 3. Due Date filter
            if (match && filterState.dueDate !== 'all') {
                if (filterState.dueDate === 'no_date') {
                    if (task.dueDate) match = false;
                } else if (!task.dueDate) {
                    match = false;
                } else {
                    const dueDate = new Date(task.dueDate);
                    const now = new Date();
                    const dueDay = new Date(dueDate.getFullYear(), dueDate.getMonth(), dueDate.getDate());
                    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
                    
                    const diffTime = dueDay - today;
                    const diffDays = Math.ceil(diffTime / (1000 * 60 * 60 * 24));
                    
                    // get day of week (0-6, 0 is Sunday, let's make 1 Monday)
                    const currentDayOfWeek = today.getDay() === 0 ? 7 : today.getDay();
                    const daysToSunday = 7 - currentDayOfWeek;

                    switch(filterState.dueDate) {
                        case 'overdue':
                            if (diffDays >= 0 || task.percentDone >= 100) match = false;
                            break;
                        case 'today':
                            if (diffDays !== 0) match = false;
                            break;
                        case 'tomorrow':
                            if (diffDays !== 1) match = false;
                            break;
                        case 'this_week':
                            if (diffDays < 0 || diffDays > daysToSunday) match = false;
                            break;
                        case 'next_week':
                            if (diffDays <= daysToSunday || diffDays > daysToSunday + 7) match = false;
                            break;
                    }
                }
            }

            return match;
        };

        const filteredTasks = computed(() => {
            if (!isFiltering.value && !filterState.globalSearch) {
                return tasks.value;
            }

            const filterTree = (list, forceKeep = false) => {
                const result = [];
                list.forEach(task => {
                    const taskMatch = checkTaskMatch(task);
                    const filteredChildren = task.children ? filterTree(task.children, forceKeep || taskMatch) : [];
                    if (taskMatch || filteredChildren.length > 0 || forceKeep) {
                        const taskCopy = { ...task, children: filteredChildren };
                        if (filteredChildren.length > 0) {
                            taskCopy.expanded = true;
                        }
                        result.push(taskCopy);
                    }
                });
                return result;
            };

            if (filterState.globalSearch) {
                const result = [];
                todoLists.value.forEach(list => {
                    const fTasks = isFiltering.value ? filterTree(list.tasks) : list.tasks;
                    if (fTasks.length > 0) {
                        result.push({
                            id: 'list_header_' + list.id,
                            title: `[列表] ${list.name}`,
                            isListHeader: true,
                            expanded: true,
                            children: fTasks,
                            percentDone: 0,
                            priority: 0,
                            dueDate: '',
                            categories: [],
                            comments: ''
                        });
                    }
                });
                return result;
            } else {
                return isFiltering.value ? filterTree(tasks.value) : tasks.value;
            }
        });

        const addTag = () => {
            const tag = newTagInput.value.trim();
            if (tag && selectedTask.value) {
                if (!selectedTask.value.categories) {
                    selectedTask.value.categories = [];
                }
                if (!selectedTask.value.categories.includes(tag)) {
                    selectedTask.value.categories.push(tag);
                }
                newTagInput.value = '';
            }
        };

        const removeTag = (index) => {
            if (selectedTask.value && selectedTask.value.categories) {
                selectedTask.value.categories.splice(index, 1);
            }
        };

        // (Select Task Logic moved up)

        // Helper to generate unique ID
        const generateId = () => {
            return Date.now().toString(36) + Math.random().toString(36).substr(2, 5);
        };

        const pad2 = (n) => String(n).padStart(2, '0');

        const nowToTodoDateTimeString = () => {
            const d = new Date();
            return `${d.getFullYear()}/${pad2(d.getMonth() + 1)}/${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
        };

        const formatTaskCreatedAt = (value) => {
            if (!value) return '—';
            return String(value).replace(/\//g, '-');
        };

        const createNewList = (name = '未命名列表') => {
            const id = generateId();
            const doc = new DOMParser().parseFromString('<?xml version="1.0" encoding="utf-16"?><TODOLIST></TODOLIST>', "text/xml");
            return {
                id,
                name,
                xmlDoc: doc,
                tasks: [],
                fileHandle: null,
                originalFileName: name + '.xml'
            };
        };

        const addNewList = () => {
            const newList = createNewList('未命名列表 ' + (todoLists.value.length + 1));
            todoLists.value.push(newList);
            activeListId.value = newList.id;
        };

        const findTaskInList = (taskList, id) => {
            for (const t of taskList) {
                if (t.id === id) return true;
                if (t.children && findTaskInList(t.children, id)) return true;
            }
            return false;
        };

        const removeList = (id) => {
            const list = todoLists.value.find(l => l.id === id);
            if (list) {
                showConfirm('关闭列表', `确定要关闭列表 "${list.name}" 吗？`, () => {
                    todoLists.value = todoLists.value.filter(l => l.id !== id);
                    if (activeListId.value === id) {
                        activeListId.value = todoLists.value.length > 0 ? todoLists.value[0].id : null;
                    }
                    if (selectedTask.value && !todoLists.value.some(l => findTaskInList(l.tasks, selectedTask.value.id))) {
                        selectedTask.value = null;
                    }
                });
            }
        };

        // Create a new task object
        const createNewTaskObject = (title = '新任务', doc) => {
            const id = generateId();
            const node = doc.createElement('TASK');
            const createdAt = nowToTodoDateTimeString();
            node.setAttribute('ID', id);
            node.setAttribute('TITLE', title);
            node.setAttribute('CREATEDATESTRING', createdAt);
            return {
                id,
                title,
                percentDone: 0,
                priority: 0,
                dueDate: '',
                createdAt,
                fileRefPath: '',
                comments: '',
                categories: [],
                children: [],
                expanded: true,
                node: node
            };
        };

        // Add Root Task
        const addTask = () => {
            if (!activeList.value) return;
            const newTask = createNewTaskObject('新任务', activeList.value.xmlDoc);
            activeList.value.tasks.push(newTask);
            selectTask(newTask);
        };

        // Add Subtask
        const addSubtask = (parentTask) => {
            const doc = parentTask.node.ownerDocument;
            const newTask = createNewTaskObject('新子任务', doc);
            parentTask.children.push(newTask);
            parentTask.expanded = true;
            selectTask(newTask);
        };

        // Delete Task (recursive helper)
        const removeTaskFromList = (list, taskId) => {
            const index = list.findIndex(t => t.id === taskId);
            if (index !== -1) {
                list.splice(index, 1);
                return true;
            }
            for (const t of list) {
                if (removeTaskFromList(t.children, taskId)) {
                    return true;
                }
            }
            return false;
        };

        const deleteTask = (task) => {
            showConfirm('删除任务', `确定要删除任务 "${task.title}" 及其所有子任务吗？`, () => {
                for (const list of todoLists.value) {
                    if (removeTaskFromList(list.tasks, task.id)) {
                        break;
                    }
                }
                if (selectedTask.value && selectedTask.value.id === task.id) {
                    selectedTask.value = null;
                }
            });
        };

        // Drag and Drop Logic
        const onDragStart = ({ event, task }) => {
            dragState.draggedTask = task;
            event.dataTransfer.effectAllowed = 'move';
            // Need to set some data to make drag work in some browsers
            event.dataTransfer.setData('text/plain', task.id);
            // Optional: set drag image or transparency
            setTimeout(() => {
                event.target.style.opacity = '0.5';
            }, 0);
        };

        const onDragOver = ({ event, task }) => {
            event.preventDefault();
            event.dataTransfer.dropEffect = 'move';
            if (dragState.draggedTask && dragState.draggedTask.id !== task.id) {
                // Prevent dropping into its own descendants
                let isDescendant = false;
                let current = task;
                // We need parent references or just check recursively. 
                // Actually easier to just check if `task` is inside `draggedTask`
                const checkDescendant = (t, targetId) => {
                    if (t.id === targetId) return true;
                    return t.children.some(child => checkDescendant(child, targetId));
                };
                if (!checkDescendant(dragState.draggedTask, task.id)) {
                    dragState.dragOverTaskId = task.id;
                }
            }
        };

        const onDrop = ({ event, task }) => {
            event.target.style.opacity = '1';
            dragState.dragOverTaskId = null;
            const dragged = dragState.draggedTask;
            
            if (!dragged || dragged.id === task.id) return;
            
            // Prevent dropping into itself or its descendants
            const checkDescendant = (t, targetId) => {
                if (t.id === targetId) return true;
                return t.children.some(child => checkDescendant(child, targetId));
            };
            if (checkDescendant(dragged, task.id)) return;

            // Remove from old position
            removeTaskFromList(tasks.value, dragged.id);

            // Determine drop position based on mouse Y relative to the element
            // Simple logic: if shift is held or near bottom, add as sibling. Else add as child.
            // Let's refine: upper 25% -> insert before, lower 25% -> insert after, middle 50% -> insert as child
            const rect = event.currentTarget.getBoundingClientRect();
            const y = event.clientY - rect.top;
            
            const insertBefore = (list, targetId, item) => {
                const index = list.findIndex(t => t.id === targetId);
                if (index !== -1) {
                    list.splice(index, 0, item);
                    return true;
                }
                for (const t of list) {
                    if (insertBefore(t.children, targetId, item)) return true;
                }
                return false;
            };

            const insertAfter = (list, targetId, item) => {
                const index = list.findIndex(t => t.id === targetId);
                if (index !== -1) {
                    list.splice(index + 1, 0, item);
                    return true;
                }
                for (const t of list) {
                    if (insertAfter(t.children, targetId, item)) return true;
                }
                return false;
            };

            if (y < rect.height * 0.25) {
                // Insert before
                insertBefore(tasks.value, task.id, dragged);
            } else if (y > rect.height * 0.75) {
                // Insert after
                insertAfter(tasks.value, task.id, dragged);
            } else {
                // Insert as child
                task.children.push(dragged);
                task.expanded = true;
            }
            
            dragState.draggedTask = null;
        };

        // Reset drag state on window drop or dragend
        window.addEventListener('dragend', (e) => {
            if (e.target) e.target.style.opacity = '1';
            dragState.draggedTask = null;
            dragState.dragOverTaskId = null;
        });

        // File operations
        const handleFileSelect = (event) => {
            const file = event.target.files[0];
            if (!file || !selectedTask.value) return;
            
            // Due to browser security, we can't get the real absolute path
            // We'll store a relative path assumption or just the file name
            // Often ToDoList expects relative paths like .\filename
            selectedTask.value.fileRefPath = '.\\' + file.name;
            
            // Reset input so the same file can be selected again
            event.target.value = '';
        };

        const openFileRef = (path) => {
            if (!path) return;
            
            // Check if it's a web URL
            if (path.startsWith('http://') || path.startsWith('https://')) {
                window.open(path, '_blank');
                return;
            }
            
            // Convert Windows relative path .\ or absolute to web relative if possible
            let webPath = path;
            if (path.startsWith('.\\') || path.startsWith('./')) {
                webPath = path.substring(2); // remove .\
                // Replace remaining backslashes with forward slashes
                webPath = webPath.replace(/\\/g, '/');
                window.open(webPath, '_blank');
                return;
            }

            // For absolute local paths like C:\ or \\server\, browsers block them.
            // Best we can do is try to open it and let the browser block it or warn the user.
            try {
                // Some browsers might allow file:/// if configured, so we try formatting it
                if (path.match(/^[a-zA-Z]:\\/) || path.startsWith('\\\\')) {
                    const fileUrl = 'file:///' + path.replace(/\\/g, '/');
                    window.open(fileUrl, '_blank');
                } else {
                    window.open(path.replace(/\\/g, '/'), '_blank');
                }
            } catch (e) {
                alert('由于浏览器安全限制，无法直接打开本地绝对路径。请手动复制路径：\n' + path);
            }
        };

        // Parse XML File
        const parseXMLText = (xmlText, listObj) => {
            const parser = new DOMParser();
            listObj.xmlDoc = parser.parseFromString(xmlText, "text/xml");
            
            const root = listObj.xmlDoc.querySelector('TODOLIST');
            if (root) {
                listObj.tasks = Array.from(root.childNodes)
                    .filter(node => node.nodeType === Node.ELEMENT_NODE && node.tagName === 'TASK')
                    .map(parseTaskNode);
            } else {
                alert('无效的 ToDoList XML 文件');
            }
        };

        const loadXML = async () => {
            try {
                if ('showOpenFilePicker' in window) {
                    const [handle] = await window.showOpenFilePicker({
                        types: [
                            {
                                description: 'XML Files',
                                accept: { 'text/xml': ['.xml'] },
                            },
                        ],
                    });
                    const file = await handle.getFile();
                    const xmlText = await file.text();
                    
                    const newList = createNewList(file.name);
                    newList.fileHandle = handle;
                    newList.originalFileName = file.name;
                    parseXMLText(xmlText, newList);
                    
                    todoLists.value.push(newList);
                    activeListId.value = newList.id;
                } else {
                    const input = document.createElement('input');
                    input.type = 'file';
                    input.accept = '.xml';
                    input.onchange = (e) => {
                        const file = e.target.files[0];
                        if (!file) return;
                        const reader = new FileReader();
                        reader.onload = (ev) => {
                            const newList = createNewList(file.name);
                            newList.fileHandle = null;
                            newList.originalFileName = file.name;
                            parseXMLText(ev.target.result, newList);
                            
                            todoLists.value.push(newList);
                            activeListId.value = newList.id;
                        };
                        reader.readAsText(file);
                    };
                    input.click();
                }
            } catch (err) {
                if (err.name !== 'AbortError') {
                    console.error('Error loading file:', err);
                    alert('加载文件时出错: ' + err.message);
                }
            }
        };

        const parseTaskNode = (node) => {
            // Find specific child elements
            let fileRefPath = '';
            let comments = '';
            const children = [];
            
            Array.from(node.childNodes).forEach(child => {
                if (child.nodeType === Node.ELEMENT_NODE) {
                    if (child.tagName === 'TASK') {
                        children.push(parseTaskNode(child));
                    } else if (child.tagName === 'FILEREFPATH') {
                        fileRefPath = child.textContent;
                    } else if (child.tagName === 'COMMENTS') {
                        comments = child.textContent;
                    }
                }
            });

            // Extract DUEDATESTRING
            let dueDate = node.getAttribute('DUEDATESTRING') || '';
            if (dueDate) {
                // ToDoList often uses YYYY-MM-DD format but with slashes, try to convert to standard YYYY-MM-DD for input[type=date]
                dueDate = dueDate.replace(/\//g, '-');
                // Ensure length is 10 (e.g., 2023-11-29)
                if (dueDate.length > 10) dueDate = dueDate.substring(0, 10);
            }

            const createdAt = (node.getAttribute('CREATEDATESTRING') || '').trim();

            // Extract CATEGORY (Tags)
            const categories = [];
            Array.from(node.childNodes).forEach(child => {
                if (child.nodeType === Node.ELEMENT_NODE && child.tagName === 'CATEGORY') {
                    if (child.textContent) {
                        categories.push(child.textContent);
                    }
                }
            });

            return {
                id: node.getAttribute('ID'),
                title: node.getAttribute('TITLE') || '',
                percentDone: parseInt(node.getAttribute('PERCENTDONE') || '0', 10),
                priority: parseInt(node.getAttribute('PRIORITY') || '0', 10),
                dueDate: dueDate,
                createdAt,
                fileRefPath: fileRefPath,
                comments: comments,
                categories: categories,
                children: children,
                expanded: true,
                node: node // Reference to original XML node
            };
        };

        // Export XML
        const updateNodeFromTask = (task, parentNode, doc) => {
            const node = task.node;
            node.setAttribute('TITLE', task.title);
            node.setAttribute('PERCENTDONE', task.percentDone.toString());
            node.setAttribute('PRIORITY', task.priority.toString());
            
            if (task.dueDate) {
                // Convert YYYY-MM-DD back to YYYY/MM/DD if preferred, or keep as is
                node.setAttribute('DUEDATESTRING', task.dueDate.replace(/-/g, '/'));
            } else {
                node.removeAttribute('DUEDATESTRING');
            }

            if (task.createdAt) {
                node.setAttribute('CREATEDATESTRING', task.createdAt);
            } else {
                node.removeAttribute('CREATEDATESTRING');
            }

            // Update FILEREFPATH
            let fileNode = Array.from(node.childNodes).find(n => n.nodeType === Node.ELEMENT_NODE && n.tagName === 'FILEREFPATH');
            if (task.fileRefPath) {
                if (!fileNode) {
                    fileNode = doc.createElement('FILEREFPATH');
                    node.appendChild(fileNode);
                }
                fileNode.textContent = task.fileRefPath;
            } else if (fileNode) {
                node.removeChild(fileNode);
            }

            // Update COMMENTS
            let commentsNode = Array.from(node.childNodes).find(n => n.nodeType === Node.ELEMENT_NODE && n.tagName === 'COMMENTS');
            if (task.comments) {
                if (!commentsNode) {
                    commentsNode = doc.createElement('COMMENTS');
                    node.appendChild(commentsNode);
                }
                commentsNode.textContent = task.comments;
                node.setAttribute('COMMENTSTYPE', 'PLAIN_TEXT');
            } else if (commentsNode) {
                node.removeChild(commentsNode);
            }

            // Update CATEGORY (Tags)
            // First remove all existing CATEGORY nodes
            Array.from(node.childNodes).forEach(child => {
                if (child.nodeType === Node.ELEMENT_NODE && child.tagName === 'CATEGORY') {
                    node.removeChild(child);
                }
            });
            // Then add current ones
            if (task.categories && task.categories.length > 0) {
                task.categories.forEach(cat => {
                    const catNode = doc.createElement('CATEGORY');
                    catNode.textContent = cat;
                    node.appendChild(catNode);
                });
            }

            // Append this node to its parent in the DOM tree. 
            // This naturally removes it from its previous location, keeping structural integrity!
            parentNode.appendChild(node);

            // Recursively update children
            task.children.forEach(child => updateNodeFromTask(child, node, doc));
        };

        const generateXMLStringForList = (list) => {
            if (!list.xmlDoc) {
                list.xmlDoc = new DOMParser().parseFromString('<?xml version="1.0" encoding="utf-16"?><TODOLIST></TODOLIST>', "text/xml");
            }

            const root = list.xmlDoc.querySelector('TODOLIST');
            if (!root) return null;

            // Clear all existing TASK elements from root to prepare for rebuilding
            // Note: we only want to clear TASK elements to avoid deleting global settings if any exist
            const taskNodes = Array.from(root.childNodes).filter(n => n.nodeType === Node.ELEMENT_NODE && n.tagName === 'TASK');
            taskNodes.forEach(n => root.removeChild(n));

            // Sync JS state back to XML DOM and rebuild tree
            list.tasks.forEach(task => updateNodeFromTask(task, root, list.xmlDoc));

            // Serialize XML
            const serializer = new XMLSerializer();
            let xmlString = serializer.serializeToString(list.xmlDoc);
            
            // Ensure there is exactly one XML declaration
            if (!xmlString.trim().startsWith('<?xml')) {
                xmlString = '<?xml version="1.0" encoding="utf-16"?>\n' + xmlString;
            }
            return xmlString;
        };

        const clearHistory = () => {
            historyStack.value = [];
            hasUncommittedChanges.value = false;
            pushToHistory();
        };

        const saveXML = async () => {
            if (!activeList.value) return;
            const xmlString = generateXMLStringForList(activeList.value);
            if (!xmlString) return;

            if ('showSaveFilePicker' in window && activeList.value.fileHandle) {
                try {
                    if ((await activeList.value.fileHandle.queryPermission({ mode: 'readwrite' })) !== 'granted') {
                        if ((await activeList.value.fileHandle.requestPermission({ mode: 'readwrite' })) !== 'granted') {
                            throw new Error('没有写入文件的权限');
                        }
                    }
                    const writable = await activeList.value.fileHandle.createWritable();
                    await writable.write(xmlString);
                    await writable.close();
                    clearHistory();
                    showToast('保存成功', 'success');
                } catch (err) {
                    console.error('Error saving file:', err);
                    showToast('保存失败: ' + err.message, 'error', 3500);
                }
            } else {
                await saveAsXML();
            }
        };

        const saveAsXML = async () => {
            if (!activeList.value) return;
            const xmlString = generateXMLStringForList(activeList.value);
            if (!xmlString) return;

            if ('showSaveFilePicker' in window) {
                try {
                    const handle = await window.showSaveFilePicker({
                        suggestedName: activeList.value.originalFileName || 'todolist.xml',
                        types: [
                            {
                                description: 'XML Files',
                                accept: { 'text/xml': ['.xml'] },
                            },
                        ],
                    });
                    const writable = await handle.createWritable();
                    await writable.write(xmlString);
                    await writable.close();
                    activeList.value.fileHandle = handle;
                    activeList.value.originalFileName = handle.name;
                    activeList.value.name = handle.name;
                    clearHistory();
                    showToast('另存为成功', 'success');
                } catch (err) {
                    if (err.name !== 'AbortError') {
                        console.error('Error saving file:', err);
                        showToast('另存为失败: ' + err.message, 'error', 3500);
                    }
                }
            } else {
                const blob = new Blob([xmlString], { type: 'text/xml;charset=utf-8' });
                const url = URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = activeList.value.originalFileName || 'todolist.xml';
                document.body.appendChild(a);
                a.click();
                document.body.removeChild(a);
                URL.revokeObjectURL(url);
                clearHistory();
                showToast('已下载 XML 文件', 'success');
            }
        };

        const serializeState = () => {
            return {
                activeListId: activeListId.value,
                lists: todoLists.value.map(list => {
                    const expandedState = {};
                    const extractExpanded = (tasks) => {
                        tasks.forEach(t => {
                            if (t.expanded) expandedState[t.id] = true;
                            if (t.children) extractExpanded(t.children);
                        });
                    };
                    if (list.tasks) extractExpanded(list.tasks);

                    const xmlText = generateXMLStringForList(list);
                    return {
                        id: list.id,
                        name: list.name,
                        xmlText: xmlText,
                        handle: toRaw(list.fileHandle),
                        originalFileName: list.originalFileName,
                        selectedTaskId: activeList.value && activeList.value.id === list.id && selectedTask.value ? selectedTask.value.id : list.selectedTaskId,
                        expandedState: expandedState
                    };
                })
            };
        };

        const saveStateToDB = debounce(async () => {
            const stateToSave = serializeState();
            await dbSet('appState', stateToSave);
        }, 1000);

        // Undo History Stack
        const historyStack = ref([]);
        const hasUncommittedChanges = ref(false);
        let isRestoring = false;
        let lastSavedStateJSON = '';

        const pushToHistory = () => {
            if (isRestoring) return;
            const state = serializeState();
            const stateJSON = JSON.stringify(state);
            
            if (stateJSON !== lastSavedStateJSON) {
                historyStack.value.push(state);
                lastSavedStateJSON = stateJSON;
                if (historyStack.value.length > 50) {
                    historyStack.value.shift();
                }
            }
            hasUncommittedChanges.value = false;
        };

        const debouncedPushToHistory = debounce(pushToHistory, 500);

        const canUndo = computed(() => historyStack.value.length > 1 || hasUncommittedChanges.value);

        const restoreState = (savedState) => {
            isRestoring = true;
            
            todoLists.value = savedState.lists.map(listData => {
                const parser = new DOMParser();
                const doc = parser.parseFromString(listData.xmlText, "text/xml");
                const root = doc.querySelector('TODOLIST');
                let parsedTasks = [];
                if (root) {
                    parsedTasks = Array.from(root.childNodes)
                        .filter(node => node.nodeType === Node.ELEMENT_NODE && node.tagName === 'TASK')
                        .map(parseTaskNode);
                        
                    if (listData.expandedState) {
                        const applyExpanded = (tasks) => {
                            tasks.forEach(t => {
                                t.expanded = !!listData.expandedState[t.id];
                                if (t.children) applyExpanded(t.children);
                            });
                        };
                        applyExpanded(parsedTasks);
                    }
                }
                return {
                    id: listData.id,
                    name: listData.name,
                    xmlDoc: doc,
                    tasks: parsedTasks,
                    fileHandle: listData.handle,
                    originalFileName: listData.originalFileName,
                    selectedTaskId: listData.selectedTaskId
                };
            });
            activeListId.value = savedState.activeListId;

            // Reset isRestoring after Vue updates
            setTimeout(() => {
                isRestoring = false;
                saveStateToDB(); // ensure DB is updated with restored state
            }, 0);
        };

        const undo = () => {
            if (!canUndo.value) return;

            let stateToRestore;

            if (hasUncommittedChanges.value) {
                // Uncommitted changes exist, restore the top of the stack
                stateToRestore = historyStack.value[historyStack.value.length - 1];
            } else {
                // Current state is already the top of the stack, pop it and restore the previous one
                historyStack.value.pop();
                stateToRestore = historyStack.value[historyStack.value.length - 1];
                lastSavedStateJSON = JSON.stringify(stateToRestore);
            }

            if (stateToRestore) {
                restoreState(stateToRestore);
                hasUncommittedChanges.value = false;
            }
        };

        onMounted(async () => {
            const savedState = await dbGet('appState');
            if (savedState && savedState.lists && savedState.lists.length > 0) {
                todoLists.value = savedState.lists.map(listData => {
                    const parser = new DOMParser();
                    const doc = parser.parseFromString(listData.xmlText, "text/xml");
                    const root = doc.querySelector('TODOLIST');
                    let parsedTasks = [];
                    if (root) {
                        parsedTasks = Array.from(root.childNodes)
                            .filter(node => node.nodeType === Node.ELEMENT_NODE && node.tagName === 'TASK')
                            .map(parseTaskNode);
                            
                        // Apply expanded state post-parsing
                        if (listData.expandedState) {
                            const applyExpanded = (tasks) => {
                                tasks.forEach(t => {
                                    t.expanded = !!listData.expandedState[t.id];
                                    if (t.children) applyExpanded(t.children);
                                });
                            };
                            applyExpanded(parsedTasks);
                        }
                    }
                    return {
                        id: listData.id,
                        name: listData.name,
                        xmlDoc: doc,
                        tasks: parsedTasks,
                        fileHandle: listData.handle,
                        originalFileName: listData.originalFileName,
                        selectedTaskId: listData.selectedTaskId
                    };
                });
                if (savedState.activeListId && todoLists.value.some(l => l.id === savedState.activeListId)) {
                    activeListId.value = savedState.activeListId;
                } else {
                    activeListId.value = todoLists.value[0].id;
                }
            } else {
                addNewList();
            }

            // Initialize history stack
            pushToHistory();

            // Set up auto-save and history tracking
            watch([todoLists, activeListId], () => {
                if (!isRestoring) {
                    hasUncommittedChanges.value = true;
                    debouncedPushToHistory();
                    saveStateToDB();
                }
            }, { deep: true });
        });

        return {
            todoLists,
            activeListId,
            activeList,
            addNewList,
            removeList,
            tasks,
            filteredTasks,
            filterState,
            hasActiveFilters,
            clearFilters,
            allAvailableTags,
            loadXML,
            saveXML,
            saveAsXML,
            undo,
            canUndo,
            selectedTask,
            selectTask,
            addTask,
            addSubtask,
            deleteTask,
            onDragStart,
            onDragOver,
            onDrop,
            handleFileSelect,
            openFileRef,
            newTagInput,
            addTag,
            removeTag,
            confirmModal,
            closeConfirm,
            toast,
            closeToast,
            formatTaskCreatedAt
        };
    }
});

app.mount('#app');
