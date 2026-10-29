// Lets expensive per-page work (text layers) wait until the user stops scrolling.
let lastScroll = 0;

export const noteScroll = () => {
  lastScroll = performance.now();
};

export const sinceScroll = () => performance.now() - lastScroll;
