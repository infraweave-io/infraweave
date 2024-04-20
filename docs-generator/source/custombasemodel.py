from pydantic import BaseModel

class CustomBaseModel(BaseModel):
    def __init__(self, **data):
        module_name = self.__class__.__name__
        print(f"Setting up module {module_name} with parameters {data}...")
        super().__init__(**data)

