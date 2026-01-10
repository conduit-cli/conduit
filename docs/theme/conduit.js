// Conduit Documentation Custom Scripts

document.addEventListener('DOMContentLoaded', function() {
  // Add external link indicators
  document.querySelectorAll('a[href^="http"]').forEach(function(link) {
    if (!link.href.includes('getconduit.sh')) {
      link.setAttribute('target', '_blank');
      link.setAttribute('rel', 'noopener noreferrer');
    }
  });

  // Add copy buttons to code blocks
  document.querySelectorAll('pre code').forEach(function(block) {
    const button = document.createElement('button');
    button.className = 'copy-button';
    button.textContent = 'Copy';
    button.addEventListener('click', function() {
      navigator.clipboard.writeText(block.textContent).then(function() {
        button.textContent = 'Copied!';
        setTimeout(function() {
          button.textContent = 'Copy';
        }, 2000);
      });
    });
    block.parentElement.style.position = 'relative';
    block.parentElement.appendChild(button);
  });
});
