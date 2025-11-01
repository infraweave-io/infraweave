"""
MkDocs plugin to automatically generate Python API documentation as markdown.
"""

import os
import sys
from pathlib import Path

from mkdocs.plugins import BasePlugin
from mkdocs.config import config_options


class PythonDocsPlugin(BasePlugin):
    """Generate Python API documentation using griffe."""
    
    config_scheme = (
        ('module_name', config_options.Type(str, default='infraweave')),
        ('output_dir', config_options.Type(str, default='docs/api')),
    )
    
    def render_class_to_markdown(self, cls, module_name):
        """Render a single class to markdown."""
        lines = []
        
        # Class header
        lines.append(f"# {cls.name}\n\n")
        if cls.docstring:
            lines.append(f"{cls.docstring.value}\n\n")
        
        # Get methods (excluding private and special methods except __init__, __enter__, __exit__)
        methods = [
            m for m in cls.members.values() 
            if m.is_function and (
                not m.name.startswith('_') or 
                m.name in ['__init__', '__enter__', '__exit__', '__new__']
            )
        ]
        
        if methods:
            lines.append(f"## Methods\n\n")
            for method in methods:
                # Build method signature
                params = [p.name for p in method.parameters if p.name not in ['self', '/']]
                sig = f"{method.name}({', '.join(params)})"
                
                lines.append(f"### `{sig}`\n\n")
                if method.docstring:
                    lines.append(f"{method.docstring.value}\n\n")
        
        return ''.join(lines)
    
    def render_index_to_markdown(self, module):
        """Render the index/overview page."""
        lines = []
        
        # Module header
        lines.append(f"# Python SDK Reference\n\n")
        if module.docstring:
            lines.append(f"{module.docstring.value}\n\n")
        
        # Get all classes
        classes = [m for m in module.members.values() if m.is_class]
        
        if classes:
            lines.append(f"## Classes\n\n")
            for cls in classes:
                # Link to the class page
                lines.append(f"### [{cls.name}]({cls.name.lower()}.md)\n\n")
                
                # Add a brief description (first line/paragraph of docstring)
                if cls.docstring:
                    # Get first paragraph
                    first_para = cls.docstring.value.split('\n\n')[0]
                    lines.append(f"{first_para}\n\n")
        
        return ''.join(lines)
    
    def on_pre_build(self, config):
        """Generate Python documentation before building documentation."""
        print("Generating Python API documentation...")
        
        module_name = self.config['module_name']
        output_dir = Path(self.config['output_dir'])
        
        # Set environment variables globally
        os.environ['PDOC_BUILD'] = '1'
        os.environ['PROVIDER'] = 'none'
        os.environ['AWS_REGION'] = 'us-east-1'
        
        # Add infraweave_py to Python path
        sys.path.insert(0, 'infraweave_py')
        
        try:
            import griffe
            
            # Load the module with griffe using dynamic inspection
            module = griffe.load(
                module_name,
                force_inspection=True,
                resolve_aliases=True
            )
            
            # Create output directory
            output_dir.mkdir(parents=True, exist_ok=True)
            
            # Get all classes
            classes = [m for m in module.members.values() if m.is_class]
            
            # Generate index page
            index_content = self.render_index_to_markdown(module)
            index_path = output_dir / 'index.md'
            index_path.write_text(index_content)
            print(f"✓ Generated index at {index_path}")
            
            # Generate individual class pages
            for cls in classes:
                class_content = self.render_class_to_markdown(cls, module_name)
                class_path = output_dir / f'{cls.name.lower()}.md'
                class_path.write_text(class_content)
                print(f"✓ Generated {cls.name} documentation at {class_path}")
            
            print(f"✓ Python API documentation generated in {output_dir}")
                
        except Exception as e:
            print(f"Error generating Python documentation: {e}")
            import traceback
            traceback.print_exc()

